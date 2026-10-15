/** The state of a screen with nothing in it yet (or nothing matching). */
export interface EmptyStateProps extends React.HTMLAttributes<HTMLDivElement>{
/** Tabler icon name @default 'inbox' */
icon?:string|null;
title?:React.ReactNode;
description?:React.ReactNode;
/** one or two Buttons */
action?:React.ReactNode;
/** sm for a panel or a filtered list, default for a whole screen @default 'md' */
size?:'sm'|'md'|'default';
}
