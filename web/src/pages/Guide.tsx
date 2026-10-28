import { Icon } from "../cryst";

interface Section {
  id: string;
  title: string;
  /** A screenshot under `public/guide/`, taken from this GUI. */
  image: string;
  alt: string;
  steps: string[];
}

const SECTIONS: Section[] = [
  {
    id: "setup",
    title: "Setup: the table and the name",
    image: "setup-table.png",
    alt: "The Setup page on the Table step, with the spot name above the table view",
    steps: [
      "Setup builds one spot in steps: Table, Start, Range, Tree and Solver. The tabs switch steps; Next goes on in order.",
      "On the Table step, set the players, stacks, blinds, antes and the economics (cash or tournament).",
      "The name above the table names the spot, the run and the saved TOML (in the run id and file name, spaces and other symbols become _). Leave it empty to use the automatic name shown as its placeholder; it follows the spot.",
      "The table view shows the replayed spot: the street, the pot, the effective stack and SPR, the board, and OOP / IP tags on the two players left.",
    ],
  },
  {
    id: "start",
    title: "Start: building the line",
    image: "setup-start.png",
    alt: "The Start step with a player's action popover open",
    steps: [
      "Press two seats on the table to pick a preflop history that leaves those two in, then pick the starting street and the postflop flow.",
      "Or build the line badge by badge: the next player's badge opens their legal actions; a bet or raise asks for the street total in BB.",
      "Press an earlier badge to go back to before that action. The line text stays editable.",
      "Choose opens the board picker. Cards past those the line deals are shown dimmed and ignored.",
    ],
  },
  {
    id: "range",
    title: "Range",
    image: "setup-range.png",
    alt: "The Range step with the OOP range on the grid",
    steps: [
      "Once the line starts a postflop street, edit the ranges of the two players left.",
      "Switch OOP and IP with the tabs; Swap OOP and IP exchanges the two ranges.",
      "Write the range text, or paint the grid at the chosen paint weight; both edit the same range.",
    ],
  },
  {
    id: "tree",
    title: "Tree: bet sizes",
    image: "setup-tree.png",
    alt: "The Tree step with the bet size table",
    steps: [
      "The size table sets bet and raise sizes per street for OOP and IP. The info button explains the size notation.",
      "The Script tab edits the tree script the table writes; a script the table cannot hold is edited there. Ctrl+Space completes keywords, variables and sizes, hovering a word explains it, and the daemon's error is marked on its line.",
      "Below the table, No donk bets and Re-raises are all-in only reshape the tree, and each script param gets a field. For an all-in at every node, start the script with add bet [a] and add raise [a].",
    ],
  },
  {
    id: "viewer",
    title: "Tree viewer",
    image: "tree-viewer.png",
    alt: "The Tree card beside the steps, listing the flop's decisions",
    steps: [
      "Once validation passes, the Tree card lists every decision on a street: who acts and the pot, then each action with its size in BB and as a share of the pot (raises also as a multiple of the bet faced).",
      "Press an action, or use the arrow keys, to open or close the decision after it; the buttons at the top right open or close them all.",
      "Deal turn and Deal river pick the next card and show that street; the streets walked stay above as cards to go back to.",
    ],
  },
  {
    id: "validation",
    title: "Validation",
    image: "validation.png",
    alt: "The Validation card with the tree size and the memory estimate",
    steps: [
      "Each edit is validated by the daemon. A problem is marked on its step tab and shown under the field; Fix in opens that step.",
      "When it passes, the card shows the tree's nodes, terminals, storage and memory estimate, and any warnings.",
      "Save effective config downloads the config with its defaults filled in.",
    ],
  },
  {
    id: "toml",
    title: "TOML",
    image: "setup-toml.png",
    alt: "The Edit TOML view",
    steps: [
      "Edit TOML writes the whole document as TOML. Each readable edit updates the steps, and step edits rewrite it. The script inside it gets the same completion and hover, and errors are marked on their line.",
      "Open TOML and Save TOML read and write the document as a file. Load template starts from a template.",
      "Solve creates a run named after the spot and opens it on Runs.",
    ],
  },
  {
    id: "runs",
    title: "Runs",
    image: "runs.png",
    alt: "The Runs page with a finished run selected",
    steps: [
      "Runs lists every run with its state. Select one for its progress, convergence chart and artifacts.",
      "Cancel a running run or resume a stopped one from its detail; Results opens a finished run.",
    ],
  },
  {
    id: "results",
    title: "Results",
    image: "results.png",
    alt: "The Results page with a strategy grid",
    steps: [
      "Results walks a finished run's tree: press an action card to go down, and pick a card at a chance node.",
      "The 13×13 grid shows the strategy, EV or range per hand class; the Hands tab lists the combos. Export saves the node's strategy as CSV.",
      "A run saved with Saved streets = no-rivers keeps no River strategies. Opening a River node solves that River on the spot from the stored reach, which can take a few seconds; moving within the same River then needs no wait, and picking a River card again solves it anew. The notice above the grid marks those strategies and EVs as recomputed, not the original solve's.",
    ],
  },
];

/** The operation guide: one screenshot per part of the GUI, with what to do there. */
export function GuidePage() {
  return (
    <>
      <header className="page-header">
        <h1>Guide</h1>
      </header>
      <main className="page-body padded guide">
        <nav className="guide-toc" aria-label="Guide sections">
          {SECTIONS.map((section) => (
            <button
              key={section.id}
              type="button"
              className="guide-toc-item"
              onClick={() => document.getElementById(`guide-${section.id}`)?.scrollIntoView({ behavior: "smooth" })}
            >
              <Icon name="chevron-right" size={14} />
              {section.title}
            </button>
          ))}
        </nav>
        <div className="guide-sections">
          {SECTIONS.map((section) => (
            <section key={section.id} id={`guide-${section.id}`} className="guide-section">
              <h2>{section.title}</h2>
              <ol className="guide-steps">
                {section.steps.map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
              <img className="guide-shot" src={`guide/${section.image}`} alt={section.alt} />
            </section>
          ))}
        </div>
      </main>
    </>
  );
}
