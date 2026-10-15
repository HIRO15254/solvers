Every action and destination in the app, one keystroke away. In a tool this is the fastest path for people who use it daily.

```jsx
<CommandPalette open={open} onOpenChange={setOpen} groups={[
  { group: 'Actions', items: [{ id:'new', label:'New record', icon:'plus', shortcut:['N'], onSelect: create }] },
  { group: 'Go to',   items: [{ id:'people', label:'People', icon:'users', shortcut:['G','P'], onSelect: goPeople }] },
]} />
```

- It registers ⌘K / Ctrl+K itself while `onOpenChange` is given; pass `hotkey={null}` to own the shortcut yourself.
- Typing filters on the label, the group and any `keywords` — add synonyms there ("csv" for Import).
- ↑ ↓ move, ↵ runs, Esc closes; the palette closes before the action runs.
- Every item should be reachable another way too; the palette is the shortcut, not the only door.
- Put the same shortcut on the item here and in the shortcut sheet, so they never drift.
