/**
 * Cryst Design System — every export of the bundle, in one place.
 *
 * The bundle is a classic script that assigns `window.Cryst`
 * (the older name `window.KoguDesignSystem_6c0df3` still points at the same object).
 * Props for each component live beside it in `components/<Name>/<Name>.d.ts`;
 * how and when to use it lives in `components/<Name>/README.md`.
 *
 * House rules these types follow:
 * - sizes: `sm` | `default` (= `md`) | `lg` on desktop controls, `xl` for touch (44px);
 *   `default` is accepted everywhere a size is taken.
 * - status: `neutral` | `info` | `success` | `warning` | `destructive`, shared by
 *   Alert, Toast and Badge, with one icon and one accent color each (`STATUS`).
 * - state callbacks: `onValueChange`, `onCheckedChange`, `onOpenChange`; native
 *   `onChange` still fires on the form controls, and every native prop passes through.
 * - each root element carries shadcn's `data-slot` name; internal class names are
 *   prefixed `cr-` and are an implementation detail.
 * - keyboard: ⌘K opens CommandPalette, ? opens ShortcutsDialog, / focuses search,
 *   ↑ ↓ (and J/K) move in a List, Esc steps back (except on an explicit BottomSheet,
 *   which only × and ✓ close). Components register their own
 *   global keys only while the matching onOpenChange is given.
 */
export as namespace Cryst;

/** Core */
export { Icon } from './Icon/Icon';
export declare const STATUS: Record<'neutral'|'info'|'success'|'warning'|'destructive',{icon:string;color:string}>;

/** Actions */
export { Button, buttonVariants } from './Button/Button';
export { IconButton } from './IconButton/IconButton';

/** Forms */
export { Input, Label } from './Input/Input';
export { Textarea } from './Textarea/Textarea';
export { Select } from './Select/Select';
export { Checkbox } from './Checkbox/Checkbox';
export { Radio, RadioGroup, RadioGroupItem } from './Radio/Radio';
export { SegmentedControl } from './SegmentedControl/SegmentedControl';
export { RadioCardGroup, RadioCard } from './RadioCard/RadioCard';
export { Switch } from './Switch/Switch';

/** Display */
export { Card, CardHeader, CardTitle, CardDescription, CardAction, CardContent, CardFooter } from './Card/Card';
export { Badge } from './Badge/Badge';
export { Avatar, AvatarImage, AvatarFallback } from './Avatar/Avatar';
export { Tag } from './Tag/Tag';
export { Kbd } from './Kbd/Kbd';
export { EmptyState } from './EmptyState/EmptyState';
export { SelectionBar } from './SelectionBar/SelectionBar';
export { FilterBar } from './FilterBar/FilterBar';
export { Alert, AlertTitle, AlertDescription } from './Alert/Alert';
export { Table, TableHeader, TableBody, TableFooter, TableRow, TableHead, TableCell, TableCaption } from './Table/Table';
export { Separator } from './Separator/Separator';
export { Skeleton } from './Skeleton/Skeleton';
export { Progress } from './Progress/Progress';

/** Navigation */
export { Tabs, TabsList, TabsTrigger, TabsContent } from './Tabs/Tabs';
export { Pagination } from './Pagination/Pagination';
export { Sidebar, SidebarSection, SidebarItem } from './Sidebar/Sidebar';
export { List, ListItem } from './List/List';

/** Overlay */
export { Dialog } from './Dialog/Dialog';
export { BottomSheet } from './BottomSheet/BottomSheet';
export { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem, DropdownMenuCheckboxItem, DropdownMenuLabel, DropdownMenuSeparator } from './DropdownMenu/DropdownMenu';
export { CommandPalette } from './CommandPalette/CommandPalette';
export { ShortcutsDialog } from './ShortcutsDialog/ShortcutsDialog';
export { Popover, PopoverTrigger, PopoverContent } from './Popover/Popover';
export { Toast, Toaster, toast } from './Toast/Toast';
export { Tooltip } from './Tooltip/Tooltip';
