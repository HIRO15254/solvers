import { readFile, writeFile } from "node:fs/promises"

const source = new URL("../icons/128x128@2x.png", import.meta.url)
const destination = new URL("../icons/icon.ico", import.meta.url)
const png = await readFile(source)
const directory = Buffer.alloc(22)

directory.writeUInt16LE(0, 0)
directory.writeUInt16LE(1, 2)
directory.writeUInt16LE(1, 4)
directory.writeUInt8(0, 6)
directory.writeUInt8(0, 7)
directory.writeUInt8(0, 8)
directory.writeUInt8(0, 9)
directory.writeUInt16LE(1, 10)
directory.writeUInt16LE(32, 12)
directory.writeUInt32LE(png.length, 14)
directory.writeUInt32LE(directory.length, 18)

await writeFile(destination, Buffer.concat([directory, png]))
