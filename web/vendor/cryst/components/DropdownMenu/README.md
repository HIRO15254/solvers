Menu of actions for the current object; opens from a button and closes when an item is chosen, on Esc, or on a click outside.

```jsx
<DropdownMenu align="end">
  <DropdownMenuTrigger><IconButton icon="dots" label="More options" /></DropdownMenuTrigger>
  <DropdownMenuContent>
    <DropdownMenuLabel>This item</DropdownMenuLabel>
    <DropdownMenuItem icon="pencil" shortcut="E" onSelect={rename}>Rename</DropdownMenuItem>
    <DropdownMenuSeparator />
    <DropdownMenuItem icon="trash" variant="destructive" onSelect={remove}>Delete</DropdownMenuItem>
  </DropdownMenuContent>
</DropdownMenu>
```

- Verb-first labels; group with a Separator; destructive actions last, in `variant="destructive"`.
- Arrow keys move between items and the first item takes focus when the menu opens.
- Use `align="end"` for menus at the right edge of a row or toolbar.
- DropdownMenu or Select? This runs an **action**; Select picks a **value** that stays in a field.
- `DropdownMenuCheckboxItem` toggles something and keeps the menu open — the columns menu of a Table is exactly this.
- DropdownMenu or Popover? A list of actions is a menu; anything else inside a panel (filters, a small form, extra detail) is a Popover.
