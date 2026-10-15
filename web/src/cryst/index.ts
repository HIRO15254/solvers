// Typed ES-module view of the vendored Cryst Design System (web/vendor/cryst).
// The bundle assigns `window.Cryst`; prop types come from its per-component
// .d.ts files. Styles: tokens.css (generated from tokens.json), bundle.css
// (the Tabler icon classes) and fonts.css (the local font files); each component
// injects its own CSS when the bundle loads.
import "./react-global";
import "../../vendor/cryst/components/bundle.js";
import "../../vendor/cryst/tokens.css";
import "../../vendor/cryst/components/bundle.css";
import "./fonts.css";

import type { ComponentType, HTMLAttributes, ReactElement, RefAttributes } from "react";
import type { AlertProps } from "../../vendor/cryst/components/Alert/Alert";
import type { BadgeProps } from "../../vendor/cryst/components/Badge/Badge";
import type { ButtonProps } from "../../vendor/cryst/components/Button/Button";
import type { CardPartProps, CardProps } from "../../vendor/cryst/components/Card/Card";
import type { CheckboxProps } from "../../vendor/cryst/components/Checkbox/Checkbox";
import type { DialogProps } from "../../vendor/cryst/components/Dialog/Dialog";
import type { EmptyStateProps } from "../../vendor/cryst/components/EmptyState/EmptyState";
import type { IconProps } from "../../vendor/cryst/components/Icon/Icon";
import type { IconButtonProps } from "../../vendor/cryst/components/IconButton/IconButton";
import type { InputProps, LabelProps } from "../../vendor/cryst/components/Input/Input";
import type { KbdProps } from "../../vendor/cryst/components/Kbd/Kbd";
import type { PopoverContentProps, PopoverProps } from "../../vendor/cryst/components/Popover/Popover";
import type { ProgressProps } from "../../vendor/cryst/components/Progress/Progress";
import type { SegmentedControlProps } from "../../vendor/cryst/components/SegmentedControl/SegmentedControl";
import type { SelectProps } from "../../vendor/cryst/components/Select/Select";
import type { SeparatorProps } from "../../vendor/cryst/components/Separator/Separator";
import type {
  SidebarItemProps,
  SidebarProps,
  SidebarSectionProps,
} from "../../vendor/cryst/components/Sidebar/Sidebar";
import type { SkeletonProps } from "../../vendor/cryst/components/Skeleton/Skeleton";
import type { SwitchProps } from "../../vendor/cryst/components/Switch/Switch";
import type {
  TableCellProps,
  TableHeadProps,
  TableProps,
  TableRowProps,
} from "../../vendor/cryst/components/Table/Table";
import type {
  TabsContentProps,
  TabsListProps,
  TabsProps,
  TabsTriggerProps,
} from "../../vendor/cryst/components/Tabs/Tabs";
import type { TextareaProps } from "../../vendor/cryst/components/Textarea/Textarea";
import type { ToasterProps, ToastOptions, ToastProps } from "../../vendor/cryst/components/Toast/Toast";
import type { TooltipProps } from "../../vendor/cryst/components/Tooltip/Tooltip";

type WithRef<P, E> = ComponentType<P & RefAttributes<E>>;
type Status = "neutral" | "info" | "success" | "warning" | "destructive";
type ToastFn = ((title: string, options?: ToastOptions) => string | number) & {
  [K in "success" | "error" | "warning" | "info"]: (title: string, options?: ToastOptions) => string | number;
} & { dismiss: (id?: string | number) => void };

interface CrystNamespace {
  STATUS: Record<Status, { icon: string; color: string }>;
  Icon: ComponentType<IconProps>;
  Button: WithRef<ButtonProps, HTMLButtonElement>;
  IconButton: WithRef<IconButtonProps, HTMLButtonElement>;
  Input: WithRef<InputProps, HTMLInputElement>;
  Label: WithRef<LabelProps, HTMLLabelElement>;
  Textarea: WithRef<TextareaProps, HTMLTextAreaElement>;
  Select: WithRef<SelectProps, HTMLSelectElement>;
  Checkbox: WithRef<CheckboxProps, HTMLInputElement>;
  Switch: WithRef<SwitchProps, HTMLInputElement>;
  SegmentedControl: ComponentType<SegmentedControlProps>;
  Card: ComponentType<CardProps>;
  CardHeader: ComponentType<CardPartProps>;
  CardTitle: ComponentType<CardPartProps>;
  CardDescription: ComponentType<CardPartProps>;
  CardAction: ComponentType<CardPartProps>;
  CardContent: ComponentType<CardPartProps>;
  CardFooter: ComponentType<CardPartProps>;
  Badge: ComponentType<BadgeProps>;
  Kbd: ComponentType<KbdProps>;
  EmptyState: ComponentType<EmptyStateProps>;
  Alert: ComponentType<AlertProps>;
  Table: ComponentType<TableProps>;
  TableHeader: ComponentType<HTMLAttributes<HTMLTableSectionElement>>;
  TableBody: ComponentType<HTMLAttributes<HTMLTableSectionElement>>;
  TableFooter: ComponentType<HTMLAttributes<HTMLTableSectionElement>>;
  TableRow: ComponentType<TableRowProps>;
  TableHead: ComponentType<TableHeadProps>;
  TableCell: ComponentType<TableCellProps>;
  TableCaption: ComponentType<HTMLAttributes<HTMLTableCaptionElement>>;
  Separator: ComponentType<SeparatorProps>;
  Skeleton: ComponentType<SkeletonProps>;
  Progress: ComponentType<ProgressProps>;
  Tabs: ComponentType<TabsProps>;
  TabsList: ComponentType<TabsListProps>;
  TabsTrigger: ComponentType<TabsTriggerProps>;
  TabsContent: ComponentType<TabsContentProps>;
  Sidebar: ComponentType<SidebarProps>;
  SidebarSection: ComponentType<SidebarSectionProps>;
  SidebarItem: ComponentType<SidebarItemProps>;
  Dialog: ComponentType<DialogProps>;
  Popover: ComponentType<PopoverProps>;
  PopoverTrigger: ComponentType<{ children: ReactElement; asChild?: boolean }>;
  PopoverContent: ComponentType<PopoverContentProps>;
  Toast: ComponentType<ToastProps>;
  Toaster: ComponentType<ToasterProps>;
  toast: ToastFn;
  Tooltip: ComponentType<TooltipProps>;
  __errors: { path: string; error: string }[];
}

declare global {
  interface Window {
    Cryst?: CrystNamespace;
  }
}

const cryst = window.Cryst;
if (!cryst) throw new Error("the Cryst bundle did not define window.Cryst");
if (cryst.__errors.length > 0) {
  console.error("Cryst bundle failed to load some components", cryst.__errors);
}

export const {
  STATUS,
  Icon,
  Button,
  IconButton,
  Input,
  Label,
  Textarea,
  Select,
  Checkbox,
  Switch,
  SegmentedControl,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardAction,
  CardContent,
  CardFooter,
  Badge,
  Kbd,
  EmptyState,
  Alert,
  Table,
  TableHeader,
  TableBody,
  TableFooter,
  TableRow,
  TableHead,
  TableCell,
  TableCaption,
  Separator,
  Skeleton,
  Progress,
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
  Sidebar,
  SidebarSection,
  SidebarItem,
  Dialog,
  Popover,
  PopoverTrigger,
  PopoverContent,
  Toast,
  Toaster,
  toast,
  Tooltip,
} = cryst;

export type { InputProps };
