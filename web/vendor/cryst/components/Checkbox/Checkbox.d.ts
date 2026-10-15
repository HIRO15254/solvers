/** Checkbox (shadcn API) with an optional inline label. Also exports Label for pairing by htmlFor/id. */
export interface CheckboxProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>,'size'|'checked'|'defaultChecked'>{
label?:string;
checked?:boolean|'indeterminate';
defaultChecked?:boolean;
onCheckedChange?:(checked:boolean)=>void;
/** xl = 44px tap row with a 20px box (mobile) @default 'md' */
size?:'md'|'xl';
}
