Switches between views of the same context: `underline` for page sections, `pill` for a compact view switch.

```jsx
<Tabs items={[{id:'all',label:'All',icon:'list',count:24},'Archived']} value={tab} onValueChange={setTab} />

<Tabs defaultValue="week" variant="pill">
  <TabsList>
    <TabsTrigger value="day">Day</TabsTrigger>
    <TabsTrigger value="week">Week</TabsTrigger>
  </TabsList>
  <TabsContent value="day">…</TabsContent>
  <TabsContent value="week">…</TabsContent>
</Tabs>

<Tabs variant="pill" size="xl" fullWidth items={['Overview','Basics','Notes']} />
```

- Composed form follows shadcn (Tabs, TabsList, TabsTrigger, TabsContent); the `items` short form renders only the tab bar.
- Pill: a `muted` track holding a `tab-active` pill. `tab-active` is white in light and a lifted gray in dark, so the selected tab stands clear of the track in both themes; never fill it with `card`, which sits darker than the track in dark.
- On phones use `size="xl"` (`m-text-secondary` labels; the pill track is `m-control` tall) and `fullWidth`, which splits the width evenly between tabs.
- Arrow keys, Home and End move between tabs.
- Tabs or SegmentedControl? Tabs swap the view below them; a SegmentedControl sets a value inside a form. If the choice is saved, it is a SegmentedControl.
