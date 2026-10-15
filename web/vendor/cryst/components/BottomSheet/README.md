The mobile container for forms, pickers and read-only panels, and the Cryst counterpart of shadcn's Drawer. It comes in two kinds, and the kind decides the whole top of the sheet: whether a grab is shown, which icons sit in the header, and what closes it.

| | Dismissible | Explicit |
|---|---|---|
| Use for | Pickers, menus, read-only panels, anything that saves as it goes | Forms that are saved as a whole, and anything where closing by accident loses input |
| Top of sheet | Grab, then the centered title | No grab. × cancel top-left, centered title, ✓ confirm top-right |
| Closes on | Dragging the grab down, a scrim tap, Esc | Only × or ✓ |
| Chosen by | the default without `onConfirm`, or `dismissible` | `onConfirm`, or `dismissible={false}` |

```jsx
<BottomSheet open={open} onOpenChange={setOpen} title="Currency">
  <List>…</List>
</BottomSheet>

<BottomSheet open={open} onOpenChange={setOpen} title="Edit event"
  onCancel={confirmDiscard} onConfirm={save} confirmDisabled={!valid} confirmLoading={saving}>
  <Input label="Amount" size="xl" />
</BottomSheet>
```

- **A grab means "safe to close at any time".** Never show one on an explicit sheet, and never make a sheet that has one wait for a confirm.
- Header controls are icons, never text: × (`x`, 24px) cancels and ✓ (`check`, 24px, `primary`) confirms, each in an `m-control` square. `cancelLabel` and `confirmLabel` become their accessible names. While `confirmLoading`, ✓ turns into a spinner, is disabled and sets `aria-busy`.
- `onCancel` replaces the default close on ×, so an explicit sheet with unsaved changes can ask first ("Discard changes?" in a Dialog) and close only when the person agrees.
- There is no footer button row; the only in-body action is a single full-width primary Button when a sheet's job is one command (e.g. "Register 5 players").
- Focus moves to the first field. Use `xl` sizes for every control inside.
- BottomSheet or Dialog? Same job, different form factor: BottomSheet on touch screens, Dialog on desktop. A destructive confirmation stays a Dialog on every screen.
