/** Anchored panel (shadcn Popover): Popover + PopoverTrigger + PopoverContent. Closes on Esc and on a click outside. */
export interface PopoverProps extends React.HTMLAttributes<HTMLSpanElement>{
open?:boolean;
defaultOpen?:boolean;
onOpenChange?:(open:boolean)=>void;
/** @default 'bottom' */
side?:'bottom'|'top';
/** @default 'start' */
align?:'start'|'end'|'center';
}
export interface PopoverContentProps extends React.HTMLAttributes<HTMLDivElement>{}
