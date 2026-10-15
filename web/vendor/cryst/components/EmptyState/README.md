One icon, one line, and the action that fills the screen.

```jsx
<EmptyState icon="inbox" title="No records yet"
  description="Records you create appear here."
  action={<Button icon="plus" size="sm">New record</Button>} />
```

- Say what would be here and how to get it; never just "No data".
- Two different empties: nothing exists yet (offer the create action) and nothing matches (offer to clear the filters) — use `size="sm"` for the second, inside the list.
- No illustrations. The icon is one Tabler glyph in `muted-foreground`.
