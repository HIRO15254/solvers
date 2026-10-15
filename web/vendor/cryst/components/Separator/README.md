One `border` hairline between groups of content.

```jsx
<Separator style={{ margin: '12px 0' }} />
<Separator orientation="vertical" />
```

- Prefer space first; add a line only where the grouping would otherwise be ambiguous.
- Vertical separators stretch to the row height, so give the row a height or align its items.
- Cards, Dialogs and Tables draw their own hairlines; don't add more inside them.
