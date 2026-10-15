/** Hover/focus tooltip with an optional shortcut hint. */
export interface TooltipProps{
content:string;
/** monospace shortcut hint, e.g. "⌘K" */
shortcut?:string;
/** @default 'top' */
side?:'top'|'bottom'|'left'|'right';
/** hover delay in ms @default 300 */
delayDuration?:number;
open?:boolean;
defaultOpen?:boolean;
onOpenChange?:(open:boolean)=>void;
children:React.ReactNode;
className?:string;
style?:React.CSSProperties;
}
