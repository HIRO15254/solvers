/** Command palette — Cryst's take on shadcn's Command / CommandDialog. */
export interface CommandPaletteProps extends React.HTMLAttributes<HTMLDivElement>{
open?:boolean;
onOpenChange?:(open:boolean)=>void;
/** [{group, items:[{id,label,icon?,shortcut?,hint?,keywords?,disabled?,onSelect}]}] */
groups?:Array<{group?:string;items:Array<{id?:string;label:string;icon?:string;shortcut?:string|string[];hint?:string;keywords?:string[];disabled?:boolean;onSelect?:(item:any)=>void}>}>;
/** @default 'Type a command or search…' */
placeholder?:string;
/** @default 'No matches' */
emptyLabel?:string;
/** key that opens it with ⌘/Ctrl; null turns the global shortcut off @default 'k' */
hotkey?:string|null;
/** replaces the hint row at the bottom */
footer?:React.ReactNode;
}
