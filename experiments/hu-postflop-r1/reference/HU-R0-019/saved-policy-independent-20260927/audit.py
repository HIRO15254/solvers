"""Read one retained SOL3 and evaluate its River policy without Rust or solver kernels.

This is a deliberately bounded research reader, not a general SOL implementation.
Its binary decoder follows the pinned historical format. Poker ranking, terminal
payoffs and the hand-pair expectation/BR walk are independent Python arithmetic.
"""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass, field
from fractions import Fraction
import hashlib
import importlib.metadata
from itertools import combinations
import json
import math
from pathlib import Path
import struct
import sys
import time
import tomllib

import blake3
import zstandard

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
CASE = HERE.parent
RAW = CASE.parent / 'evidence-vm06-river/records/river/diagnostic019'
HIST = REPO / 'experiments/hu-postflop-r1/validation/vm07-complete/records/build/current'
SOL_SHA = '986af722a0ef15c5d7a5b58715a1c0026ba8dbac99edff6a2986b02e98b24a8b'
MAX_BYTES = 4 * 1024 * 1024
DEADLINE = math.inf


def require(value, message):
    if not value:
        raise ValueError(message)


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


class Wire:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def take(self, count):
        require(0 <= count <= MAX_BYTES and self.pos + count <= len(self.data), 'truncated wire')
        result = self.data[self.pos:self.pos + count]
        self.pos += count
        return result

    def uint(self, bits=64):
        value = 0
        for index in range((bits + 6) // 7):
            byte = self.take(1)[0]
            value |= (byte & 127) << (7 * index)
            if byte < 128:
                require(value < 1 << bits and (index == 0 or byte != 0), 'noncanonical/overflow varint')
                return value
        raise ValueError('unterminated varint')

    def blob(self):
        return self.take(self.uint())

    def text(self):
        return self.blob().decode('utf-8')

    def real(self, fmt='d'):
        value = struct.unpack('<' + fmt, self.take(struct.calcsize(fmt)))[0]
        require(math.isfinite(value), 'nonfinite wire float')
        return value

    def finish(self):
        require(self.pos == len(self.data), 'trailing postcard bytes')


def _section(data, offset, compressed, decoded, digest):
    require(0 < compressed <= MAX_BYTES and 0 < decoded <= MAX_BYTES, 'section size bound')
    require(0 <= offset <= len(data) and offset + compressed <= len(data), 'section outside file')
    encoded = data[offset:offset + compressed]
    try:
        parameters = zstandard.get_frame_parameters(encoded)
        require(parameters.window_size <= 64 * 1024 * 1024 and parameters.dict_id == 0,
                'zstd window/dictionary bound')
        require(parameters.content_size in (zstandard.CONTENTSIZE_UNKNOWN, decoded), 'zstd content size differs')
        raw = zstandard.ZstdDecompressor(max_window_size=64 * 1024 * 1024).decompress(
            encoded, max_output_size=decoded, allow_extra_data=False)
    except zstandard.ZstdError as exc:
        raise ValueError('invalid zstd section') from exc
    require(len(raw) == decoded, 'decoded section length differs')
    require(blake3.blake3(raw).digest() == digest, 'section BLAKE3 differs')
    return raw


def decode_sol(data):
    require(106 <= len(data) <= MAX_BYTES, 'file size bound')
    require(data[:8] == b'SLVRSOLV' and struct.unpack_from('<H', data, 8)[0] == 3, 'requires SOL3')
    iteration = struct.unpack_from('<Q', data, 42)[0]
    compressed, decoded = struct.unpack_from('<QQ', data, 50)
    digest = data[66:98]
    count = struct.unpack_from('<Q', data, 98)[0]
    directory = 106 + compressed
    end = directory + 64 * count
    require(0 < count <= 64 and end <= len(data), 'directory outside file')
    wire = Wire(_section(data, 106, compressed, decoded, digest))
    config = wire.text()
    meta = {'iterations': wire.uint(), 'expl': [wire.real(), wire.real()],
            'ev': [wire.real(), wire.real()], 'nash_conv': wire.real(),
            'storage': wire.text(), 'wall_secs': wire.real()}
    mode, node_count, stored_nodes = wire.uint(32), wire.uint(), wire.uint()
    wire.finish()
    require(blake3.blake3(config.encode()).digest() == data[10:42], 'config BLAKE3 differs')
    require(iteration == meta['iterations'], 'iterations differ')
    require(mode == 0, 'Full policy required')
    require(meta['storage'] in ('f32', 'i16') and meta['wall_secs'] >= 0, 'invalid metadata')
    require(count <= stored_nodes <= node_count <= 4096, 'metadata count bound')
    blocks, previous = {}, -1
    for index in range(count):
        entry = directory + 64 * index
        first, last, nodes, reserved, offset, size, raw_size = struct.unpack_from('<IIIIQII', data, entry)
        require(1 <= nodes <= 64 and last == first + nodes - 1 and previous < first,
                'unordered directory')
        require(last < node_count and reserved == 0 and offset == end, 'invalid directory extent')
        raw = _section(data, offset, size, raw_size, data[entry + 32:entry + 64])
        chunk = Wire(raw)
        require(chunk.uint() == nodes, 'chunk count differs')
        for sref in range(first, last + 1):
            require(chunk.uint(32) == sref, 'strategy sref differs')
            probabilities = chunk.blob()
            require(len(probabilities) % 2 == 0, 'odd u16 blob')
            require(chunk.uint(32) == sref, 'value sref differs')
            scale, values = chunk.real('f'), chunk.blob()
            require(scale >= 0 and len(values) % 2 == 0, 'invalid values')
            blocks[sref] = {'q': struct.unpack('<' + 'H' * (len(probabilities) // 2), probabilities),
                            'scale': scale, 'values': values}
        chunk.finish()
        previous, end = last, offset + size
    require(end == len(data) and len(blocks) == stored_nodes, 'unindexed bytes or nodes')
    return {'config_text': config, 'meta': meta, 'blocks': blocks, 'node_count': node_count,
            'stored_nodes': stored_nodes, 'version': 3, 'mode': 'Full'}


def decode_probabilities(q, actions, hands):
    require(actions > 0 and hands > 0 and len(q) == actions * hands, 'policy shape differs')
    require(all(isinstance(x, int) and 0 <= x <= 65535 for x in q), 'invalid u16')
    rows = [[0.0] * hands for _ in range(actions)]
    for hand in range(hands):
        total = sum(q[action * hands + hand] for action in range(actions))
        for action in range(actions):
            rows[action][hand] = f32(f32(q[action * hands + hand]) / f32(total)) if total else f32(1 / actions)
    return rows


def card(text):
    require(len(text) == 2 and text[0] in '23456789TJQKA' and text[1] in 'cdhs', 'invalid card')
    return 4 * '23456789TJQKA'.index(text[0]) + 'cdhs'.index(text[1])


def combo_id(a, b):
    require(0 <= a < 52 and 0 <= b < 52 and a != b, 'invalid combo')
    hi, lo = max(a, b), min(a, b)
    return hi * (hi - 1) // 2 + lo


def rank5(cards):
    require(len(cards) == 5 and len(set(cards)) == 5, 'five distinct cards required')
    ranks = [c // 4 + 2 for c in cards]
    groups = sorted(((count, rank) for rank, count in Counter(ranks).items()), reverse=True)
    descending = sorted(set(ranks), reverse=True)
    straight = 5 if descending == [14, 5, 4, 3, 2] else (
        descending[0] if len(descending) == 5 and descending[0] - descending[-1] == 4 else 0)
    flush = len({c % 4 for c in cards}) == 1
    if flush and straight:
        return (8, straight)
    if groups[0][0] == 4:
        return (7, groups[0][1], groups[1][1])
    if [x[0] for x in groups] == [3, 2]:
        return (6, groups[0][1], groups[1][1])
    if flush:
        return (5, *sorted(ranks, reverse=True))
    if straight:
        return (4, straight)
    if groups[0][0] == 3:
        return (3, groups[0][1], *sorted((r for c, r in groups[1:]), reverse=True))
    if [x[0] for x in groups[:2]] == [2, 2]:
        return (2, max(groups[0][1], groups[1][1]), min(groups[0][1], groups[1][1]), groups[2][1])
    if groups[0][0] == 2:
        return (1, groups[0][1], *sorted((r for c, r in groups[1:]), reverse=True))
    return (0, *sorted(ranks, reverse=True))


def rank7(cards):
    require(len(cards) == 7 and len(set(cards)) == 7 and all(0 <= c < 52 for c in cards),
            'seven distinct deck cards required')
    return max(rank5(hand) for hand in combinations(cards, 5))


def terminal_payoff(pot, contrib, winner, rate, cap):
    require(pot >= 0 and len(contrib) == 2 and min(contrib) >= 0 and winner in (None, 0, 1), 'payoff input')
    total = pot + sum(contrib)
    rake = min(Fraction(total) * rate, cap)
    require(0 <= rake <= total, 'invalid rake')
    shares = (Fraction(1, 2), Fraction(1, 2)) if winner is None else (
        (Fraction(1), Fraction(0)) if winner == 0 else (Fraction(0), Fraction(1)))
    return tuple(shares[p] * (total - rake) - contrib[p] for p in (0, 1))


@dataclass
class Node:
    history: str = ''
    actor: int | None = None
    actions: list[str] = field(default_factory=list)
    children: list['Node'] = field(default_factory=list)
    contrib: tuple[int, int] = (0, 0)
    folder: int | None = None
    sref: int | None = None


def action_token(action):
    if action == 'check':
        return 'x'
    if action == 'fold':
        return 'f'
    if action == 'call':
        return 'c'
    require(action.startswith('bet ') or action.startswith('raise to '), 'unsupported action')
    return 'r' + str(int(action.split()[-1]))


def build_tree(menus, pot, stack):
    by_history = {row['history']: row for row in menus}
    require(len(by_history) == len(menus) and '' in by_history, 'duplicate/missing menu')
    nodes, terminals, visited = [], [], set()

    def walk(history, actor, contrib, previous_check):
        require(history in by_history and history not in visited, 'missing/revisited decision')
        visited.add(history)
        row = by_history[history]
        require(row['actor'] == ('oop', 'ip')[actor] and row['pot'] == pot + sum(contrib), 'menu state differs')
        require(row['street'] == 'river' and row['stored'] is True and row['actions'], 'unstored/nonriver menu')
        require(len(set(row['actions'])) == len(row['actions']), 'duplicate action')
        node = Node(history, actor, list(row['actions']), [], contrib, None, len(nodes))
        nodes.append(node)  # Historical storage_refs are allocated in action preorder.
        for action in node.actions:
            token = action_token(action)
            child_history = history + token
            new_contrib = list(contrib)
            folder, terminal = None, False
            outstanding = contrib[1 - actor] - contrib[actor]
            if action == 'fold':
                require(outstanding > 0, 'fold without outstanding bet')
                folder, terminal = actor, True
            elif action == 'call':
                require(outstanding > 0, 'call without outstanding bet')
                new_contrib[actor] = contrib[1 - actor]
                terminal = True
            elif action == 'check':
                require(outstanding == 0, 'check facing bet')
                terminal = previous_check
            else:
                amount = int(action.split()[-1])
                require(max(contrib) < amount <= stack, 'invalid cumulative wager')
                require(action.startswith('bet ') == (outstanding == 0), 'bet/raise mismatch')
                new_contrib[actor] = amount
            if terminal:
                require(child_history not in by_history, 'terminal is also a decision')
                child = Node(history=child_history, contrib=tuple(new_contrib), folder=folder)
                terminals.append(child)
            else:
                child = walk(child_history, 1 - actor, tuple(new_contrib), action == 'check')
            node.children.append(child)
        return node

    root = walk('', 0, (0, 0), False)
    require(visited == set(by_history), 'unreachable menu')
    return root, nodes, terminals


def evaluate_player(root, policies, hand_pairs, ranges, ranks, pot, rate, cap, player, br):
    """Counterfactual opponent-reach recursion; hero max follows opponent sum.

    Own earlier action probabilities are excluded from BR reach. Every positive
    initial hand remains present at every decision, even if its saved own reach
    is zero there. Root weights are product weights conditioned on compatibility.
    """
    opponent = 1 - player
    own, other = hand_pairs[player], hand_pairs[opponent]
    valid = [[not set(h).intersection(k) for k in other] for h in own]
    compatible_mass = [math.fsum(ranges[opponent][j] for j in range(len(other)) if row[j]) for row in valid]
    normalizer = math.fsum(w * mass for w, mass in zip(ranges[player], compatible_mass))
    require(normalizer > 0, 'no compatible joint mass')
    per_hand = []
    for i, hand in enumerate(own):
        require(time.monotonic() < DEADLINE, 'evaluation deadline exceeded')
        own_id = combo_id(*hand)

        def walk(node, reach):
            if node.actor is None:
                utilities = [float(terminal_payoff(pot, node.contrib, outcome, rate, cap)[player])
                             for outcome in (0, 1, None)]
                def term(j):
                    if node.folder is not None:
                        outcome = 1 - node.folder
                    elif ranks[player][i] == ranks[opponent][j]:
                        outcome = 2
                    else:
                        outcome = player if ranks[player][i] > ranks[opponent][j] else opponent
                    return reach[j] * utilities[outcome]
                return math.fsum(term(j) for j in range(len(other)) if valid[i][j])
            rows = policies[node.sref]
            if node.actor == player:
                values = [walk(child, reach) for child in node.children]
                return max(values) if br else math.fsum(rows[a][own_id] * value for a, value in enumerate(values))
            return math.fsum(walk(child, [r * rows[a][combo_id(*other[j])] for j, r in enumerate(reach)])
                             for a, child in enumerate(node.children))

        per_hand.append(walk(root, ranges[opponent]))
    value = math.fsum(w * result for w, result in zip(ranges[player], per_hand)) / normalizer
    return {'value': value, 'normalizer': normalizer, 'per_hand': per_hand,
            'conditional_per_hand': [v / m if m else None for v, m in zip(per_hand, compatible_mass)]}


def parse_explicit_range(text, board):
    result = {}
    for token in text.split(','):
        combo, weight = (part.strip() for part in token.split(':'))
        require(len(combo) == 4, 'requires explicit weighted combos')
        hand = (card(combo[:2]), card(combo[2:]))
        index = combo_id(*hand)
        value = f32(float(weight))
        require(index not in result and not set(hand).intersection(board), 'duplicate/blocked initial combo')
        require(math.isfinite(value) and 0 < value <= 1, 'requires positive finite initial weight')
        result[index] = (hand, value)
    return [result[index][0] for index in sorted(result)], [result[index][1] for index in sorted(result)]


def identity(path):
    data = path.read_bytes()
    return {'path': path.relative_to(REPO).as_posix(), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def human_action(text):
    if text in ('check', 'fold', 'call'):
        return text
    require(text.startswith(('bet ', 'raise to ', 'allin ')), 'unsupported observed action')
    amount = Fraction(text.split()[-1]) * 100
    require(amount.denominator == 1, 'fractional chip menu')
    return ('raise to ' if text.startswith('raise') else 'bet ') + str(int(amount))


def human_history(text):
    return ''.join(action_token(human_action(part)) for part in text.split(' / ')) if text else ''


def normalized_observation(observed):
    rows = {}
    for row in observed['observed_menus']:
        actions = []
        for action in row['actions']:
            converted = human_action(action)
            if action.startswith('allin') and row['history'] and not row['history'].endswith('check'):
                converted = converted.replace('bet ', 'raise to ')
            actions.append(converted)
        rows[human_history(row['history'])] = {'actor': {'BB': 'oop', 'BTN': 'ip'}[row['actor']], 'actions': actions}
    return rows


def crosscheck_payoffs(terminals, table, pot, rate, cap):
    """Check independent arithmetic against prior exact terminal evidence only.

    The saved i16 values and prior table never feed the expectation/BR evaluator.
    """
    prior = {human_history(row['history']): row for row in table['terminals']}
    require(len(prior) == len(terminals) and set(prior) == {n.history for n in terminals}, 'terminal table coverage')
    checked = 0
    for node in terminals:
        row = prior[node.history]
        require(row['kind'] == ('fold' if node.folder is not None else 'showdown'), 'terminal kind differs')
        require(tuple(int(row['contribution_from_root']['chips'][seat]) for seat in ('BB', 'BTN')) == node.contrib,
                'terminal contribution differs')
        require(row['folder'] == (('BB', 'BTN')[node.folder] if node.folder is not None else None), 'folder differs')
        for outcome in row['outcomes']:
            winner = 1 - node.folder if node.folder is not None else {'win_BB': 0, 'tie': None, 'win_BTN': 1}[outcome['outcome']]
            expected = tuple(Fraction(outcome['public_subgame_utility']['chips'][seat]) for seat in ('BB', 'BTN'))
            require(terminal_payoff(pot, node.contrib, winner, rate, cap) == expected, 'terminal utility differs')
            checked += 1
    return checked


def write_new(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2, allow_nan=False)
        handle.write('\n')


def run(out, policy_out, pins_out, dependency_out, seconds):
    global DEADLINE
    started = time.monotonic()
    DEADLINE = started + seconds
    # Prefer the durable first capture. The ignored pip report is needed only
    # before that receipt exists; a replay never requires it.
    retained_dependencies = HERE / 'dependencies.json'
    dependency_receipt = retained_dependencies if retained_dependencies.is_file() else REPO / '.cache/r1-policy-audit-deps.json'
    paths = [RAW / 'run/solution.sol', RAW / 'run/checkpoint.ckpt', RAW / 'run/run.toml', RAW / 'tree.json',
             RAW / 'summary.json', RAW / 'execution.json', CASE / 'observed.json', CASE / 'oop-range.txt',
             CASE / 'ip-range.txt', CASE / 'payoff-audit-20260927/terminal-table.json', Path(__file__),
             HERE / 'test_audit.py', dependency_receipt]
    paths += [HIST / name for name in ('crates/formats/src/sol.rs', 'crates/formats/src/sol_indexed.rs',
              'crates/cards/src/card.rs', 'crates/cards/src/range.rs', 'crates/engine/src/tree.rs',
              'crates/holdem/src/postflop.rs', 'crates/cli/src/sol.rs', 'crates/cli/src/postflop_artifact.rs')]
    pins = [identity(path) for path in paths]
    require(pins[0]['sha256'] == SOL_SHA and pins[0]['bytes'] == 10895, 'unexpected diagnostic SOL')
    decoded = decode_sol(paths[0].read_bytes())
    require(decoded['config_text'].encode() == (RAW / 'run/run.toml').read_bytes(), 'stored config differs')
    config = tomllib.loads(decoded['config_text'])
    game, rake = config['game'], config['rake']
    require(config['schema'] == 'solvers.postflop/v1' and config['utility'] == {'kind': 'chip-ev'}, 'unsupported game')
    require(rake == {'kind': 'percent-cap', 'rate': 0.05, 'cap': 60.0, 'no_flop_no_drop': False}, 'rake scope changed')
    board = [card(c) for c in game['board'].split()]
    require(len(set(board)) == 5 and game['pot'] == 4050 and game['effective_stack'] == 5500, '019 economics changed')
    rate, cap = Fraction(str(rake['rate'])), Fraction(str(rake['cap']))
    pairs, weights = [], []
    for seat, filename in (('oop', 'oop-range.txt'), ('ip', 'ip-range.txt')):
        require(game[seat + '_range'] == (CASE / filename).read_text().strip(), 'range text differs')
        hands, mass = parse_explicit_range(game[seat + '_range'], board)
        pairs.append(hands)
        weights.append(mass)
    require(list(map(len, pairs)) == [130, 115], '019 support changed')
    menus = json.loads((RAW / 'tree.json').read_bytes())
    observed = json.loads((CASE / 'observed.json').read_bytes())
    require(observed['case_id'] == 'HU-R0-019' and observed['board'] == game['board'].split(), 'observed board differs')
    observation = normalized_observation(observed)
    require({row['history']: {'actor': row['actor'], 'actions': row['actions']} for row in menus} == observation,
            'retained tree/observed menus differ')
    root, nodes, terminals = build_tree(menus, game['pot'], game['effective_stack'])
    require((len(nodes), len(terminals), decoded['node_count']) == (12, 21, 33), '019 topology changed')
    require(set(decoded['blocks']) == set(range(len(nodes))), 'policy node coverage differs')
    payoff_checks = crosscheck_payoffs(terminals, json.loads((CASE / 'payoff-audit-20260927/terminal-table.json').read_bytes()),
                                      game['pot'], rate, cap)
    policies, full = {}, []
    for node in nodes:
        block = decoded['blocks'][node.sref]
        require(len(block['values']) == 2 * 1326 * 2, 'value shape differs')
        rows = decode_probabilities(block['q'], len(node.actions), 1326)
        policies[node.sref] = rows
        full.append({'sref': node.sref, 'history': node.history, 'actor': node.actor, 'actions': node.actions,
                     'hand_count': 1326, 'q_action_major': block['q'], 'decoded_f32_action_rows': rows})
    # Rank every initial positive hand independently by all 21 five-card subsets.
    ranks = [[rank7(board + list(hand)) for hand in seat] for seat in pairs]
    results = {}
    for variant in ('decoded_f32', 'binary64_renormalized'):
        selected = policies
        if variant == 'binary64_renormalized':
            selected = {}
            for sref, rows in policies.items():
                sums = [math.fsum(row[h] for row in rows) for h in range(1326)]
                selected[sref] = [[p / sums[h] for h, p in enumerate(row)] for row in rows]
        records = []
        for p in (0, 1):
            records.append({name: evaluate_player(root, selected, pairs, weights, ranks, game['pot'], rate, cap, p, br)
                            for name, br in (('ev', False), ('br', True))})
        ev = [record['ev']['value'] for record in records]
        best = [record['br']['value'] for record in records]
        gains = [b - e for b, e in zip(best, ev)]
        results[variant] = {'ev_public_chips': ev, 'br_public_chips': best, 'gain_chips': gains,
                            'nash_conv_chips': math.fsum(gains), 'ev_sum_minus_3990_chips': math.fsum(ev) - 3990,
                            'normalizer': records[0]['ev']['normalizer'], 'per_seat': records}
    summary = json.loads((RAW / 'summary.json').read_bytes())
    require(summary['ev_oop'] == decoded['meta']['ev'][0] and summary['ev_ip'] == decoded['meta']['ev'][1]
            and summary['expl_oop'] == decoded['meta']['expl'][0] and summary['expl_ip'] == decoded['meta']['expl'][1]
            and summary['nash_conv'] == decoded['meta']['nash_conv'], 'summary/live SOL metadata differs')
    deps = json.loads(dependency_receipt.read_bytes())
    packages = deps['packages'] if dependency_receipt == retained_dependencies else [
        {'name': x['metadata']['name'], 'version': x['metadata']['version'], 'download': x['download_info']}
        for x in deps['install'] if x['metadata']['name'] in ('zstandard', 'blake3')]
    require({x['name']: x['version'] for x in packages} == {'zstandard': '0.25.0', 'blake3': '1.0.9'}
            and len(packages) == 2, 'dependency receipt versions differ')
    dependencies = {'python': sys.version, 'executable': sys.executable, 'packages': packages,
                    'receipt_source': identity(dependency_receipt)}
    for name, version in (('zstandard', '0.25.0'), ('blake3', '1.0.9')):
        require(importlib.metadata.version(name) == version, 'dependency version differs')
    require(pins == [identity(path) for path in paths], 'input/source changed during audit')
    report = {'schema': 'r1.019-independent-saved-policy/v1', 'state': 'completed', 'case_id': 'HU-R0-019',
              'source_scope': 'Historical source03 retained SOL3; not a current-production solve.',
              'counts': {'decisions': len(nodes), 'terminals': len(terminals), 'all_policy_hands_per_node': 1326,
                         'retained_policy_probabilities': sum(len(x['q_action_major']) for x in full),
                         'positive_initial_hands': list(map(len, pairs)),
                         'exact_terminal_outcomes_crosschecked': payoff_checks,
                         'compatible_hand_pairs': sum(not set(a).intersection(b) for a in pairs[0] for b in pairs[1])},
              'initial_ranges': [[{'combo_id': combo_id(*hand), 'cards': list(hand), 'weight_f32': weight, 'rank': rank}
                                  for hand, weight, rank in zip(pairs[p], weights[p], ranks[p])] for p in (0, 1)],
              'arithmetic': 'Decode q/sum(q) with binary32 rounding as historical SOL3. Evaluate in binary64 with math.fsum; no Rust evaluator, CFV, saved values or sorted sweep. BR maximizes after opponent expectation. Secondary normalization measures policy-column roundoff only; neither variant replicates solver f32 traversal.',
              'results': results, 'live_metadata_only': decoded['meta'],
              'saved_minus_live_chips': {'ev': [results['decoded_f32']['ev_public_chips'][p] - decoded['meta']['ev'][p] for p in (0, 1)],
                                        'nash_conv': results['decoded_f32']['nash_conv_chips'] - decoded['meta']['nash_conv']},
              'max_decoded_column_sum_error': max(abs(math.fsum(row[h] for row in rows) - 1) for rows in policies.values() for h in range(1326)),
              'elapsed_seconds': time.monotonic() - started, 'deadline_seconds': seconds,
              'external_condition_match': 'unverified', 'quality_acceptance': None,
              'limitations': ['First independent saved-policy evaluation; no pre-existing saved EV/BR target.',
                              'Live metadata is pre-u16-quantization and a different arithmetic path; agreement is not a pass criterion.',
                              'Assumes diagnostic cap60 rake at every terminal. External rake/version/accuracy remain unconfirmed.',
                              'All 1326 policy columns decoded; root expectations integrate every positive initial hand, including zero downstream own reach.',
                              'No new solve, source compilation, cloud action or cfr-ref/production code execution.']}
    write_new(policy_out, {'schema': 'r1.019-full-decoded-policy/v1', 'sol_sha256': SOL_SHA, 'global_combo_order': 'hi*(hi-1)/2+lo; card=4*rank+suit, rank2..A suitcdhs', 'nodes': full})
    write_new(pins_out, {'schema': 'r1.019-independent-source-pins/v1', 'files': pins})
    write_new(dependency_out, dependencies)
    report['artifacts'] = [identity(path) for path in (policy_out, pins_out, dependency_out)]
    write_new(out, report)
    print(json.dumps({'state': report['state'], 'elapsed_seconds': report['elapsed_seconds'], 'results': {
        name: {k: v for k, v in record.items() if k != 'per_seat'} for name, record in results.items()}}, allow_nan=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--policy-out', type=Path, required=True)
    parser.add_argument('--pins-out', type=Path, required=True)
    parser.add_argument('--dependencies-out', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=30)
    args = parser.parse_args()
    require(0 < args.seconds <= 45, 'deadline must be at most45 seconds')
    targets = [p.resolve() for p in (args.out, args.policy_out, args.pins_out, args.dependencies_out)]
    require(len(set(targets)) == 4 and all(not p.exists() and p.parent == HERE for p in targets), 'new local outputs required')
    run(*targets, args.seconds)
