One sheet listing every shortcut the app has, opened with `?`.

```jsx
<ShortcutsDialog open={open} onOpenChange={setOpen} groups={[
  { name: 'Global', items: [{ label: 'Command palette', keys: ['⌘','K'] }, { label: 'Search', keys: '/' }] },
  { name: 'Lists',  items: [{ label: 'Move down / up', keys: ['J','K'] }, { label: 'Open row', keys: '↵' }] },
]} />
```

- It listens for `?` itself, and ignores it while a field has focus.
- Group by where the shortcut applies (Global, Lists, the record type, Editing), and keep the list to what the app really binds.
- This is the source of truth for the conventions in the guide; when you add a binding, add it here in the same change.
