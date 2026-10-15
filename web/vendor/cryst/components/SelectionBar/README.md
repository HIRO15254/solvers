What can be done to the rows a person has selected.

```jsx
<SelectionBar count={selected.length} onClear={() => setSelected([])}
  actions={<><Button size="sm" variant="outline" icon="archive">Archive</Button>
            <Button size="sm" variant="destructive" icon="trash">Delete</Button></>} />
```

- Pair it with a header Checkbox in `checked="indeterminate"` when some rows are selected.
- Put it directly above the list, in the toolbar's place; it announces the count politely to screen readers.
- Destructive actions last, and confirm them in a Dialog before they run.
- The consumer owns the selection; this only renders it.
