/** Bordered surface. Short form: title/description/icon/footer. Composed (shadcn): CardHeader, CardTitle, CardDescription, CardAction, CardContent, CardFooter. */
export interface CardProps extends React.HTMLAttributes<HTMLDivElement>{
title?:string;
description?:string;
/** Tabler icon before the title */
icon?:string;
/** footer content, right-aligned (usually Buttons) */
footer?:React.ReactNode;
}
export interface CardPartProps extends React.HTMLAttributes<HTMLElement>{}
