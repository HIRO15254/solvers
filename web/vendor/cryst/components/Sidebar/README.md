The navigation column of a desktop tool: destinations, counts, and the current place.

```jsx
<Sidebar header={<Logo />} footer={<SidebarItem icon="settings">Settings</SidebarItem>}>
  <SidebarItem icon="home" active>Overview</SidebarItem>
  <SidebarItem icon="inbox" count={12}>Inbox</SidebarItem>
  <SidebarSection label="Workspace">
    <SidebarItem icon="users">People</SidebarItem>
  </SidebarSection>
</Sidebar>
```

- `sidebar-width` (244px) expanded, `sidebar-collapsed-width` (56px) collapsed to icons; the item keeps its label as a tooltip-style `title` when collapsed.
- The active item sets `aria-current="page"`; keep exactly one.
- It hides itself below 720px — on phones the bottom tab bar and the `m-` scale take over.
- Counts are for things needing attention (unread, failing), not totals.
