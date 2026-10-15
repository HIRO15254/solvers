Rows of records with aligned columns. The wrapper scrolls horizontally, so tables survive narrow windows.

```jsx
<Table>
  <TableHeader>
    <TableRow>
      <TableHead onSort={sortByName} sorted={sort}>Account</TableHead>
      <TableHead numeric>Seats</TableHead>
    </TableRow>
  </TableHeader>
  <TableBody>
    <TableRow selected={id === current}>
      <TableCell>Aiko Tanaka</TableCell>
      <TableCell numeric>12</TableCell>
    </TableRow>
  </TableBody>
</Table>
```

- Put figures in `numeric` cells: right-aligned mono with tabular figures, so digits line up.
- Header cells are uppercase `overline` on `muted`; give sortable columns `onSort` and `sorted` so the arrow and `aria-sort` stay in step.
- Status belongs in a Badge, row actions in a ghost IconButton or DropdownMenu at the end of the row.
- On mobile, prefer a list of rows over a table; a table only works there if it has two or three columns.
- Around it: a header Checkbox (`checked="indeterminate"` when some rows are picked) with SelectionBar for bulk actions, FilterBar above, Pagination below, a DropdownMenu of `DropdownMenuCheckboxItem`s for column visibility, and EmptyState when nothing matches.
- The consumer provides the data, sorting and selection; the component only renders.
