Modal dialog for confirmations and short forms on desktop; on mobile, use BottomSheet.

```jsx
<Dialog open={open} onOpenChange={setOpen} title="Delete this file?"
        description="This cannot be undone." icon="alert-triangle"
        footer={<><Button variant="outline" onClick={close}>Cancel</Button><Button variant="destructive">Delete</Button></>} />
```

- Which overlay? Dialog **blocks** the task until it is answered. A panel that can be ignored is a Popover, a list of actions is a DropdownMenu, a one-line hint is a Tooltip, and on mobile a form is a BottomSheet.
- shadcn-style `open` and `onOpenChange`; Esc, the × button and a scrim click close it. Focus moves into the panel and returns on close.
- Deviation: shadcn composes DialogContent, DialogHeader and DialogFooter; Cryst takes title, description and footer as props.
