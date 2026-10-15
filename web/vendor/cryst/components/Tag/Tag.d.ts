/** Bordered label chip with optional color dot, icon and remove button (a Cryst addition; not in shadcn/ui). */
export interface TagProps extends React.HTMLAttributes<HTMLSpanElement>{
/** Tabler icon before the text */
icon?:string;
/** CSS color for a leading dot */
color?:string;
/** shows an × button when provided */
onRemove?:()=>void;
/** accessible label of the × button @default 'Remove' */
removeLabel?:string;
}
