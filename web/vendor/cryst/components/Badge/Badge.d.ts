/** Small pill for status and counts. shadcn variants (default, secondary, destructive, outline) plus Cryst's semantic tints. */
export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement>{
/** @default 'neutral' */
variant?:'default'|'secondary'|'outline'|'neutral'|'primary'|'success'|'warning'|'destructive'|'info';
/** Tabler icon before the text */
icon?:string;
/** leading status dot @default false */
dot?:boolean;
/** hairline outline instead of tint (same as variant="outline") */
outline?:boolean;
asChild?:boolean;
}
