/** Square icon-only button (shadcn: Button size="icon") with a required accessible label. */
export interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>{
/** Tabler icon name */
icon:string;
/** accessible label, also shown as the native title */
label:string;
/** @default 'ghost' */
variant?:'ghost'|'outline'|'secondary';
/** @default 'md'; xl = 44px mobile size */
size?:'sm'|'md'|'lg'|'xl';
/** pressed / selected state (sets aria-pressed) */
active?:boolean;
}
