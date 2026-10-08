"""Keep compact SIMD evidence from the linked, measured executable."""
from pathlib import Path
import hashlib
import subprocess

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]
rustc = Path(subprocess.check_output(['rustup', 'which', 'rustc'], text=True).strip())
objdump = rustc.parent.parent / 'lib/rustlib/x86_64-pc-windows-msvc/bin/llvm-objdump.exe'
binary = ROOT / 'target/new/release/examples/p1_bench.exe'
argv = [str(objdump), '-d', '--no-show-raw-insn', str(binary)]
assembly = subprocess.check_output(argv, text=True)
(ROOT / 'runs/i16-kernels/linked-final.asm').write_text(assembly, encoding='utf-8')
lines = assembly.splitlines()
snippets = []
for index, line in enumerate(lines):
    if 'vpmovsxwd' in line:
        body = '\n'.join(lines[index:index + 24])
        if body.count('vmulps') >= 2 and 'vaddps' in body and 'vmaxps' in body:
            snippets.append(('fused dequantize / discount / add / lane maxima', body))
            break
for index, line in enumerate(lines):
    if 'vcvttps2dq' in line or 'vcvttpd2dq' in line:
        body = '\n'.join(lines[max(0, index - 12):index + 20])
        if ('vroundps' in body or 'vroundpd' in body) and ('vpack' in body or 'vpshufb' in body):
            snippets.append(('reciprocal multiply / round / packed conversion / narrowing', body))
            break
if len(snippets) != 2:
    raise RuntimeError('Expected linked SIMD loops not found')
output = '\n'.join(argv) + '\nsha256: ' + hashlib.sha256(binary.read_bytes()).hexdigest() + '\n\n'
output += '\n\n'.join(label + '\n' + body for label, body in snippets) + '\n'
(EXP / 'raw/assembly.txt').write_text(output, encoding='utf-8')
print(output)
