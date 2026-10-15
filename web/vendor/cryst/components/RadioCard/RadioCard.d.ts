/** Pick one of a few rich options: cards with icon, title, description and trailing meta. A radiogroup: one tab stop, arrow keys move and select. */
export interface RadioCardGroupProps extends Omit<React.HTMLAttributes<HTMLDivElement>,'defaultValue'|'onChange'>{
value?:string;
defaultValue?:string;
onValueChange?:(value:string)=>void;
/** xl = m-list-row cards (mobile) @default 'md' */
size?:'md'|'default'|'xl';
/** @default 1 */
columns?:number;
disabled?:boolean;
/** accessible name of the group */
label?:string;
}
export interface RadioCardProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>,'value'|'title'>{
value:string;
icon?:string;
title:React.ReactNode;
description?:React.ReactNode;
/** trailing value, set in mono tabular figures */
meta?:React.ReactNode;
}
