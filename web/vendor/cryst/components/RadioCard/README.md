Picks one of a few rich options, each a card with an icon, a title, an optional description and trailing meta such as a price or a count. The selected card is outlined in `primary` (a 2px ring, so it does not rely on color alone) on a `selection` tint.

```jsx
<RadioCardGroup label="Purchase" value={id} onValueChange={setId} size="xl">
  <RadioCard value="rebuy" icon="coin" title="Rebuy" description="30,000 chips" meta="¥5,000" />
  <RadioCard value="addon" icon="coin" title="Add-on" description="50,000 chips" meta="¥5,000" />
</RadioCardGroup>
```

- A `radiogroup` of `radio` buttons: one tab stop, arrow keys move and select. Give the group a `label`.
- `size="xl"` on phones: `m-list-row` cards with `m-text-secondary` titles and `m-text-footnote` descriptions.
- `columns` lays the cards out in an even grid (default one column).
- RadioGroup, RadioCard or SegmentedControl? Plain labels in a list → RadioGroup. Two to four one-word choices side by side → SegmentedControl. Options that need a description or a value to compare → RadioCard.
