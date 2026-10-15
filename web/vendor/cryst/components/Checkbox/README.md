Checkbox for multi-select lists and settings that need explicit opt-in; shadcn API.

```jsx
<Checkbox label="Email me about updates" checked={on} onCheckedChange={setOn} />
<Checkbox label="Select all" checked="indeterminate" />
<Checkbox id="terms" /> <Label htmlFor="terms">Accept terms</Label>
```

- `checked` accepts `true`, `false` or `'indeterminate'`; `onCheckedChange` receives a boolean. Native `onChange` also fires.
- Use `size="xl"` for 44px tap rows on mobile.
