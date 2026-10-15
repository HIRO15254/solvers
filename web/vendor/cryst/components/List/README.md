The list view of a tool: one row per record, reachable without the mouse.

```jsx
<List label="Invoices" vimKeys>
  {rows.map(r => (
    <ListItem key={r.id} icon={r.icon} title={r.title} meta={r.due}
      selected={r.id === current} onActivate={() => open(r.id)}
      actions={<IconButton icon="dots" label="More" size="sm" />} />
  ))}
</List>
```

- ↑ ↓ move, Home and End jump, Enter or Space opens the focused row; `vimKeys` adds J and K.
- Rows are 38px on desktop and `m-list-row` (44px) on touch; actions appear on hover and stay visible on touch.
- List or Table? A list when each record reads as one line; a Table when columns need comparing.
- Put a Checkbox in `leading` for multi-select and pair it with SelectionBar.
