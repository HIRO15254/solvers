Action button, API-compatible with shadcn/ui Button (variant, size, asChild, buttonVariants).

```jsx
<Button icon="plus">Create</Button>
<Button variant="outline">Cancel</Button>
<Button variant="destructive" icon="trash">Delete</Button>
<Button size="icon" variant="outline" aria-label="Settings"><Icon name="settings" /></Button>
<Button asChild variant="link"><a href="/help">Learn more</a></Button>
```

- Use one `default` (primary) button per view; pair it with `outline` for the dismissive action.
- `secondary` is a quiet filled button; `ghost` is for toolbars; `link` is for inline actions.
- Sizes: `sm` 28, `default` 32, `lg` 36, `icon` 32×32; use `xl` (44px) on mobile.
- Labels are short and verb-first; add a leading `icon` when it helps scanning.
- The consumer provides the label (children) and handlers; all native button props pass through.
- Button or IconButton? For an icon-only button prefer **IconButton**, which requires the accessible label. `size="icon"` exists so shadcn code drops in; when you use it, pass `aria-label` yourself.
