A message that belongs to the page and stays until the situation changes.

```jsx
<Alert variant="warning" title="Card expires this month"
       description="Update it to avoid interruption."
       actions={<Button size="sm" variant="outline">Update card</Button>} />
```

- Alert or Toast? Alert is a **state of the page** and stays until the situation changes; Toast is the **result of an action** the person just took and disappears. If it would still be true after a reload, it is an Alert.
- Neither one is for a single invalid field: that is the Input `error` prop, next to the field.
- Place it above the content it concerns, never as a floating banner over everything.
- Variants share one vocabulary with Toast and Badge (`neutral`, `info`, `success`, `warning`, `destructive`): same icon, same accent color. `warning` and `destructive` are announced as alerts to screen readers; the rest as status.
- One line of title plus, at most, one line of description. Long explanations belong in the content.
