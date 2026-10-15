/** Single-line text field. Every native input prop passes through (shadcn Input); label, hint, error and icon are Cryst conveniences. */
export interface InputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>,'size'>{
label?:string;
/** helper text under the field */
hint?:string;
/** error message; sets aria-invalid and the error border */
error?:string;
/** leading Tabler icon name */
icon?:string;
/** control height: sm 28 / default 32 / lg 36 / xl 44px (mobile forms) @default 'md' */
size?:'sm'|'md'|'default'|'lg'|'xl';
}
/** shadcn Label */
export interface LabelProps extends React.LabelHTMLAttributes<HTMLLabelElement>{}
