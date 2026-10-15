Bordered surface that groups related content or settings.

```jsx
<Card title="Workspace" description="Name and visibility" icon="building"
      footer={<><Button variant="outline" size="sm">Cancel</Button><Button size="sm">Save</Button></>}>
  …
</Card>

<Card>
  <CardHeader>
    <CardTitle>Storage</CardTitle>
    <CardDescription>12.4 GB of 20 GB used</CardDescription>
    <CardAction><Button variant="link" size="sm">Manage</Button></CardAction>
  </CardHeader>
  <CardContent>…</CardContent>
  <CardFooter><Button size="sm">Upgrade</Button></CardFooter>
</Card>
```

- Short form (props) wraps children in padded content; composed form (shadcn parts) leaves layout to the parts.
- Footers align actions to the right, primary action last.
