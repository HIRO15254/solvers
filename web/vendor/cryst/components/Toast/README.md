Short, non-blocking feedback about the result of an action.

```jsx
// once, near the app root
<Toaster position="bottom-right" />   // "bottom-center" on mobile

toast.success('Changes saved', { description: 'Your profile is up to date' });
toast.error('Upload failed');
toast('Link copied', { action: <Button variant="ghost" size="sm">Undo</Button> });
```

- Toast or Alert? Toast is the **result of an action** and disappears; a condition that stays true (an expiring card, a degraded service) belongs in an Alert on the page. A message worth keeping should not be a Toast.
- The `toast()` API mirrors shadcn's Sonner; `Toast` renders one card directly when you manage the list yourself.
- Variants share one vocabulary with Alert and Badge (`neutral`, `info`, `success`, `warning`, `destructive`): same icon, same accent color. Warning and destructive toasts are announced as alerts; the rest as status.
- Toasts dismiss after 4 seconds; pass `duration: Infinity` for ones that need an action.
