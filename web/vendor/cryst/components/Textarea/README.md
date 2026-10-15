Multi-line text: notes, messages, descriptions.

```jsx
<Textarea label="Message" placeholder="Write a note…" hint="Markdown is supported" />
<Textarea label="Bio" value={bio} onChange={e => setBio(e.target.value)} maxLength={120} showCount />
```

- Size `rows` to the expected answer; it stays resizable vertically.
- `showCount` needs `maxLength` and a controlled value.
- Use `size="xl"` inside a BottomSheet and mobile forms.
