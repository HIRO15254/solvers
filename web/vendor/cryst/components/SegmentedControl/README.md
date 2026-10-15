Picks one value from two to four short options shown side by side, inside a form: equal-width pills, the selected one outlined in `primary` on a `selection` tint.

```jsx
<SegmentedControl label="Direction" options={['Buy','Sell']} value={dir} onValueChange={setDir} size="xl" />

<SegmentedControl label="Ante" size="xl" value={ante} onValueChange={setAnte}
  options={[{value:'none',label:'None'},{value:'all',label:'All'},{value:'bb',label:'BB'}]} />

<SegmentedControl fit size="sm" options={['Keep','Replace']} value={choice} onValueChange={setChoice} />
```

- A `radiogroup` of `radio` buttons: one tab stop, arrow keys move and select, Home and End jump to the ends. Give it a `label` (or `aria-labelledby`) that names the field.
- Sizes: `sm` (`control-sm`) inside dense rows, `md` (`control-md`) on desktop, `xl` (`m-control`, `m-text-secondary`) on phones.
- `fit` sizes each pill to its label instead of splitting the width; use it for an inline choice inside a list row.
- Labels are one or two words; an optional `icon` sits before the label. More than four options, or options that need a description, are a RadioCard group or a Select.
- SegmentedControl or Tabs? A SegmentedControl sets a value that gets saved; Tabs switch the view. They are styled differently on purpose (outlined pills vs a track), so the two never read as each other.
