/** Round avatar with image and initials fallback (shadcn Avatar). Short form via props, or compose AvatarImage + AvatarFallback. */
export interface AvatarProps extends React.HTMLAttributes<HTMLSpanElement>{
/** image URL; the fallback shows until it loads or if it fails */
src?:string;
alt?:string;
/** initials or short text */
fallback?:React.ReactNode;
/** px @default 32 */
size?:number;
}
export interface AvatarImageProps extends React.ImgHTMLAttributes<HTMLImageElement>{src:string;}
export interface AvatarFallbackProps extends React.HTMLAttributes<HTMLSpanElement>{}
