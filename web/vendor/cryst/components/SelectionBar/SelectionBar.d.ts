/** Bar shown while rows are selected, holding the actions for them. */
export interface SelectionBarProps extends React.HTMLAttributes<HTMLDivElement>{
/** how many rows are selected */
count?:number;
/** override the default "N selected" text */
label?:React.ReactNode;
onClear?:()=>void;
/** @default 'Clear selection' */
clearLabel?:string;
/** Buttons, right-aligned */
actions?:React.ReactNode;
/** render nothing at count 0 @default true */
hideWhenEmpty?:boolean;
}
