Round avatar showing an image, or initials while the image loads or when it fails.

```jsx
<Avatar src={user.photo} alt={user.name} fallback="AK" />
<Avatar size={40}>
  <AvatarImage src={url} alt="" />
  <AvatarFallback>AK</AvatarFallback>
</Avatar>
```

- Sizes are px: 20 in dense rows, 32 by default, 40 on mobile.
- The consumer provides the image URL and one or two initials.
- Fallbacks use `primary` on the `selection` tint.
