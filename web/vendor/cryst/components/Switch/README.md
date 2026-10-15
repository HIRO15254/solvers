Toggle switch for settings that apply immediately (no save button); shadcn API.

```jsx
<Switch label="Dark mode" checked={dark} onCheckedChange={setDark} />
<Switch size="xl" label="Push notifications" defaultChecked />
```

- Use Checkbox instead when the change waits for a Save action.
