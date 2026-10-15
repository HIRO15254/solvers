/** Hairline divider (shadcn Separator). */
export interface SeparatorProps extends React.HTMLAttributes<HTMLDivElement>{
/** @default 'horizontal' */
orientation?:'horizontal'|'vertical';
/** false when it separates named regions, so screen readers announce it @default true */
decorative?:boolean;
}
