/** Pick one of 2–4 short options shown side by side, inside a form. A radiogroup: one tab stop, arrow keys move and select. */
export interface SegmentedControlProps extends Omit<React.HTMLAttributes<HTMLDivElement>,'defaultValue'|'onChange'>{
/** strings or {value,label,icon?,disabled?} */
options:Array<string|{value:string;label:string;icon?:string;disabled?:boolean}>;
value?:string;
/** @default the first option */
defaultValue?:string;
onValueChange?:(value:string)=>void;
/** sm = control-sm, md = control-md, xl = m-control (mobile) @default 'md' */
size?:'sm'|'md'|'default'|'xl';
/** size each pill to its label instead of splitting the width evenly */
fit?:boolean;
disabled?:boolean;
/** accessible name of the group */
label?:string;
}
