/** Action menu (shadcn DropdownMenu). Compose Trigger + Content with Item/Label/Separator, or pass `items` to the Content. */
export interface DropdownMenuProps extends React.HTMLAttributes<HTMLSpanElement>{
open?:boolean;
defaultOpen?:boolean;
onOpenChange?:(open:boolean)=>void;
/** @default 'bottom' */
side?:'bottom'|'top';
/** which edge the menu lines up with @default 'start' */
align?:'start'|'end';
}
export interface DropdownMenuContentProps extends React.HTMLAttributes<HTMLDivElement>{
/** shorthand: {label, icon?, shortcut?, disabled?, variant?, onSelect?} or {separator:true} */
items?:Array<{label?:string;icon?:string;shortcut?:string;disabled?:boolean;variant?:'default'|'destructive';onSelect?:()=>void;separator?:boolean;header?:boolean}>;
}
export interface DropdownMenuItemProps extends React.ButtonHTMLAttributes<HTMLButtonElement>{
/** Tabler icon name */
icon?:string;
/** right-aligned mono hint, e.g. "⌘D" */
shortcut?:string;
/** @default 'default' */
variant?:'default'|'destructive';
onSelect?:(e:React.MouseEvent)=>void;
}
export interface DropdownMenuCheckboxItemProps extends React.ButtonHTMLAttributes<HTMLButtonElement>{
checked?:boolean;
onCheckedChange?:(checked:boolean)=>void;
}
