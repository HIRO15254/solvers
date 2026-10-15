/** Loading placeholder (shadcn Skeleton). */
export interface SkeletonProps extends React.HTMLAttributes<HTMLSpanElement>{
width?:number|string;
height?:number|string;
/** round, for avatars */
circle?:boolean;
/** one line of text height */
text?:boolean;
}
