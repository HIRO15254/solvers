Shows a keyboard shortcut. Use it wherever a shortcut exists, so people learn them by seeing them.

```jsx
<Kbd keys={['⌘','K']} />
<Kbd keys={['Esc']} />
```

- Write modifiers as symbols on Apple platforms (⌘ ⇧ ⌥ ⌃) and words elsewhere (Ctrl, Shift, Alt); pick per platform, don't mix in one line.
- Sequences ("G then O") are separate caps in order; combinations are too — the caps sit next to each other.
- Tooltip takes a `shortcut` prop and DropdownMenuItem a `shortcut`; both render this component, so you rarely place it by hand outside the shortcut sheet.
