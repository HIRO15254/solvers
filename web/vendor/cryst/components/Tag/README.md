Label chip for filters and metadata (categories, people, places); removable when `onRemove` is set. A Cryst addition; shadcn/ui has no Tag.

```jsx
<Tag color="#4ea7fc">Design</Tag>
<Tag icon="user">Aiko</Tag>
<Tag onRemove={() => remove(id)}>archived</Tag>
```

- Tag or Badge? Tag is a label a person attached (and can remove); Badge is a state the system computes.
- Set `removeLabel` to a localized string when the UI is not in English.
