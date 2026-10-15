How far along a task is, or that one is running at all.

```jsx
<Progress value={62} label="Storage used" />
<Progress value={null} label="Working" />
```

- Always pass `label`: the bar has no visible text of its own.
- Put the number next to the bar in `meta` when the exact value matters.
- `warning` and `destructive` suit quotas that are nearly full or over.
- Progress or Skeleton? Progress is for work with a length (an upload, a quota) or a refresh of content already on screen; Skeleton is for the first load of a screen that has nothing to show yet.
