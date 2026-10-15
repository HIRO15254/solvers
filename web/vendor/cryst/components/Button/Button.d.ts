/** Action button. Follows shadcn/ui Button: variant, size, asChild, and every native button prop. */
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>{
/** 'default' and 'primary' are the same filled accent. @default 'default' */
variant?:'default'|'primary'|'secondary'|'outline'|'ghost'|'destructive'|'link';
/** default/md 32px · sm 28 · lg 36 · icon 32×32 · xl 44px (mobile). @default 'default' */
size?:'default'|'md'|'sm'|'lg'|'icon'|'xl';
/** render the single child element (e.g. a link) with button styling */
asChild?:boolean;
/** leading Tabler icon name */
icon?:string;
/** trailing Tabler icon name */
iconRight?:string;
fullWidth?:boolean;
}
/** Class names for a button look on any element: buttonVariants({variant, size, className}). */
export declare function buttonVariants(o?:{variant?:ButtonProps['variant'];size?:ButtonProps['size'];className?:string}):string;
