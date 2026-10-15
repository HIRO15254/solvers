Pick-one options; use RadioGroup (shadcn API) for 2–5 visible choices and Select beyond that.

```jsx
<RadioGroup value={view} onValueChange={setView} orientation="horizontal">
  <RadioGroupItem value="list" label="List" />
  <RadioGroupItem value="grid" label="Grid" />
</RadioGroup>
```

- RadioGroup handles the shared `name`, value and keyboard behavior; `size="xl"` on the group sizes every item for mobile.
- A standalone `Radio` with `name`, `checked` and `onChange` still works.
