Moves between pages of a list or table.

```jsx
<Pagination page={page} pageCount={12} onPageChange={setPage} />
```

- The consumer owns the page state and the data fetching; the component renders the controls and calls `onPageChange`.
- Long runs of pages truncate with an ellipsis; `siblings` sets how many neighbours stay visible.
- Use `size="xl"` on mobile, or swap pagination for "Load more" on touch screens.
- Localize `labels` when the UI is not in English; the numbers are already language-neutral.
