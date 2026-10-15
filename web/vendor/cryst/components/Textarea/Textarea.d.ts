/** Multi-line text field (shadcn Textarea) with the same label/hint/error conveniences as Input. */
export interface TextareaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement>{
label?:string;
hint?:string;
/** error message; sets aria-invalid and the error border */
error?:string;
/** xl = the mobile size (15px text, larger padding) @default 'md' */
size?:'md'|'xl';
/** show "n / maxLength" under the field (needs maxLength and a controlled value) */
showCount?:boolean;
}
