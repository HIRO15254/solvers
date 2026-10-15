/** Toggle switch for settings that apply immediately (shadcn API). 28×16, or a 40×24 track in a 44px tap row at xl. */
export interface SwitchProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>,'size'|'checked'|'defaultChecked'>{
label?:string;
checked?:boolean;
defaultChecked?:boolean;
onCheckedChange?:(checked:boolean)=>void;
/** xl = 44px tap row with a 40×24 track (mobile) @default 'md' */
size?:'md'|'xl';
}
