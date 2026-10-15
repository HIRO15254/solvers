/** Native select styled to the system (shadcn's Select is a Radix popover; this keeps native pickers, which suit mobile). */
export interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>,'size'>{
label?:string;
/** strings or {value,label,disabled}; <option> children also work */
options?:Array<string|{value:string;label:string;disabled?:boolean}>;
value?:string;
defaultValue?:string;
onValueChange?:(value:string)=>void;
placeholder?:string;
/** control height: sm 28 / default 32 / lg 36 / xl 44px (mobile forms) @default 'md' */
size?:'sm'|'md'|'default'|'lg'|'xl';
}
