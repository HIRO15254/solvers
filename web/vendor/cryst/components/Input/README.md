Single-line text field for forms and toolbars; every native input prop passes through, as in shadcn Input.

```jsx
<Input label="Project name" placeholder="e.g. Spring launch" hint="Visible to all members" />
<Input icon="search" placeholder="Search…" size="sm" aria-label="Search" />
<Input label="Email" type="email" error="This email is already in use" />
<Label htmlFor="pw">Password</Label> <Input id="pw" type="password" />
```

- `label`, `hint` and `error` wire up `htmlFor`, `aria-describedby` and `aria-invalid` for you; or pair a bare Input with `Label`.
- Sizes: `sm` 28, `default` 32, `lg` 36, `xl` 40 (mobile forms and BottomSheet).
- A search field without a visible label needs `aria-label`.
- One invalid field is the `error` prop, right here. Alert is for a problem with the whole page or form; Toast is for the result of submitting it.
