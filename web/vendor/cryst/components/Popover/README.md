Small panel anchored to its trigger, for filters, quick edits and extra detail.

```jsx
<Popover>
  <PopoverTrigger><Button variant="outline" icon="filter">Filter</Button></PopoverTrigger>
  <PopoverContent style={{ width: 220 }}>…</PopoverContent>
</Popover>
```

- Set the width on the content; it has no fixed size.
- Use it for optional, dismissible content. A decision that blocks the task belongs in a Dialog; a single hint belongs in a Tooltip.
- On mobile, prefer a BottomSheet: anchored panels are hard to reach and easy to dismiss by accident.
