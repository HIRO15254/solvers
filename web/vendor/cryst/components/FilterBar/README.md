Every filter in force, visible and removable, so a list is never mysteriously short.

```jsx
<FilterBar filters={filters} onRemove={remove} onClear={clearAll}>
  <Button size="sm" variant="ghost" icon="plus">Filter</Button>
</FilterBar>
```

- Each chip reads "field value" ("Status Active"); the field is muted, the value is not.
- The control that adds a filter goes in `children` — usually a ghost Button opening a Popover or DropdownMenu.
- Keep the chips above the list, under the toolbar. When a filter empties the list, the EmptyState offers to clear it.
