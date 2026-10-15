/** Radio buttons. shadcn: <RadioGroup value onValueChange><RadioGroupItem value label/></RadioGroup>. A standalone Radio still works with name/checked/onChange. */
export interface RadioGroupProps extends React.HTMLAttributes<HTMLDivElement>{
value?:string;
defaultValue?:string;
onValueChange?:(value:string)=>void;
name?:string;
disabled?:boolean;
/** @default 'vertical' */
orientation?:'vertical'|'horizontal';
/** item size for every child @default 'md' */
size?:'md'|'xl';
}
export interface RadioProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>,'size'>{
label?:string;
value?:string;
/** xl = 44px tap row with a 20px circle (mobile) @default 'md' */
size?:'md'|'xl';
}
