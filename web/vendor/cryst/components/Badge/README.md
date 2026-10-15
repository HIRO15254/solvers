Small pill for status, categories and counts.

```jsx
<Badge variant="default">New</Badge>
<Badge variant="success" icon="circle-check">Active</Badge>
<Badge variant="warning" dot>Pending</Badge>
<Badge variant="outline">Draft</Badge>
```

- shadcn variants: `default` (solid accent), `secondary`, `destructive`, `outline`.
- Status tints: `neutral`, `info`, `success`, `warning`, `destructive` — the same vocabulary, icons and accents as Alert and Toast, plus `primary` for a neutral highlight. Pair a status color with an icon or a word.
- Badge or Tag? Badge is a state the system computes (Active, Failed); Tag is a label a person attached, and can be removed.
- Deviation: `destructive` is a tint (as in the rest of the status set), not shadcn's solid red.
- Badges never take an action. If it can be clicked, it is a Button, a Tag with `onRemove`, or a filter control.
