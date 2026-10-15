Dropdown for pick-one fields, built on the native select so phones show their own picker.

```jsx
<Select label="Role" value={role} onValueChange={setRole}
        options={[{value:'viewer',label:'Viewer'},{value:'editor',label:'Editor'}]} />
<Select size="xl" placeholder="Choose a region" options={['Tokyo','Osaka']} />
```

- shadcn-style `value`, `defaultValue` and `onValueChange`; native `onChange` also fires. `<option>` children work too.
- Select or DropdownMenu? Select picks a **value** that stays in a field; DropdownMenu runs an **action**. If the chosen thing appears in the control afterwards, it is a Select.
- Deviation: shadcn's Select is a custom Radix popover (SelectTrigger, SelectContent, SelectItem); Cryst keeps the native control.
