Placeholder that holds the shape of content while it loads.

```jsx
<Skeleton circle width={32} height={32} />
<Skeleton text width="40%" />
```

- Mirror the real layout: same sizes and positions, so nothing jumps when the data arrives.
- Use it for the first load of a screen. For a refresh of content already on screen, keep the old content and show Progress instead.
- Three or four lines are enough; a full page of skeletons reads as broken. It fades in place and stops for reduced-motion viewers.
