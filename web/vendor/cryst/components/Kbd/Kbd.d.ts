/** Keyboard shortcut hint. Give it `keys` for separate key caps, or a string child. */
export interface KbdProps extends React.HTMLAttributes<HTMLElement>{
/** one cap per key, e.g. ['⌘','K'] */
keys?:string[];
/** no caps, just mono text — for dense rows */
plain?:boolean;
}
