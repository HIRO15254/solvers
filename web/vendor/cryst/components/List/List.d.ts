/** Rows people move through with the keyboard. List + ListItem. */
export interface ListProps extends React.HTMLAttributes<HTMLDivElement>{
/** rows are options in a listbox rather than plain list items */
selectable?:boolean;
/** also move with J and K @default false */
vimKeys?:boolean;
/** accessible name */
label?:string;
}
export interface ListItemProps extends React.HTMLAttributes<HTMLDivElement>{
/** leading Tabler icon */
icon?:string;
/** CSS color for that icon, usually a status color */
iconColor?:string;
title?:React.ReactNode;
/** right-aligned metadata, muted */
meta?:React.ReactNode;
/** IconButtons or a DropdownMenu; shown on hover, always on touch */
actions?:React.ReactNode;
/** something before the icon, e.g. a Checkbox */
leading?:React.ReactNode;
selected?:boolean;
disabled?:boolean;
/** click, Enter or Space */
onActivate?:(e:React.SyntheticEvent)=>void;
}
