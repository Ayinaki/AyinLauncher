/**
 * Minecraft legacy formatting-code support (the `§X` codes servers and world
 * names embed). Parses a raw string into styled segments so UI text can be
 * rendered "just like in the client".
 *
 * The color table below is the full set — the 16 classic Java colors plus the
 * Bedrock-exclusive material colors (§g–§w). Note the standard formatting
 * codes §k (obfuscated), §l (bold), §o (italic) and §r (reset) are also
 * handled. In Java Edition §m is strikethrough and §n is underline; the table
 * here follows the Bedrock color interpretation provided by the product owner
 * (material_redstone / material_copper).
 */

export interface MinecraftSegment {
	text: string
	color?: string
	bold?: boolean
	italic?: boolean
	underline?: boolean
	strikethrough?: boolean
	obfuscated?: boolean
}

/** Foreground hex per code character (lowercase). */
export const MINECRAFT_COLOR_CODES: Record<string, string> = {
	'0': '#000000',
	'1': '#0000AA',
	'2': '#00AA00',
	'3': '#00AAAA',
	'4': '#AA0000',
	'5': '#AA00AA',
	'6': '#FFAA00',
	'7': '#AAAAAA',
	'8': '#555555',
	'9': '#5555FF',
	a: '#55FF55',
	b: '#55FFFF',
	c: '#FF5555',
	d: '#FF55FF',
	e: '#FFFF55',
	f: '#FFFFFF',
	// Bedrock-exclusive material colors
	g: '#DDD605', // minecoin_gold
	h: '#E3D4D1', // material_quartz
	i: '#CECACA', // material_iron
	j: '#443A3B', // material_netherite
	m: '#971607', // material_redstone
	n: '#B4684D', // material_copper
	p: '#DEB12D', // material_gold
	q: '#11A036', // material_emerald (RGB 17,160,54 — the table's hex column had a typo)
	s: '#2CBAA8', // material_diamond
	t: '#21497B', // material_lapis
	u: '#9A5CC6', // material_amethyst
	v: '#EB7114', // material_resin
	w: '#8BB3FF', // light_blue
}

const FORMAT_CODES: Record<string, 'bold' | 'italic' | 'obfuscated'> = {
	k: 'obfuscated',
	l: 'bold',
	o: 'italic',
}

/** True when the character is a recognized code (color, formatting, or reset). */
function isCode(code: string): boolean {
	return code in MINECRAFT_COLOR_CODES || code in FORMAT_CODES || code === 'r'
}

/**
 * Parses a string containing `§X` codes into styled text segments.
 * Unknown codes (e.g. `§z`) and a trailing `§` are kept as literal text,
 * matching the client's behavior of leaving them visible.
 */
export function parseMinecraftFormatting(input: string): MinecraftSegment[] {
	const segments: MinecraftSegment[] = []

	let buffer = ''
	let color: string | undefined
	let bold = false
	let italic = false
	let underline = false
	let strikethrough = false
	let obfuscated = false

	const flush = () => {
		if (buffer.length > 0) {
			segments.push({ text: buffer, color, bold, italic, underline, strikethrough, obfuscated })
			buffer = ''
		}
	}

	for (let i = 0; i < input.length; i++) {
		const char = input[i]
		if (char === '§' && i + 1 < input.length) {
			const code = input[i + 1].toLowerCase()
			if (isCode(code)) {
				flush()
				if (code in MINECRAFT_COLOR_CODES) {
					color = MINECRAFT_COLOR_CODES[code]
				} else if (code === 'r') {
					color = undefined
					bold = false
					italic = false
					underline = false
					strikethrough = false
					obfuscated = false
				} else if (FORMAT_CODES[code]) {
					switch (FORMAT_CODES[code]) {
						case 'bold':
							bold = true
							break
						case 'italic':
							italic = true
							break
						case 'obfuscated':
							obfuscated = true
							break
					}
				}
				i++ // consume the code character
				continue
			}
		}
		buffer += char
	}
	flush()

	return segments
}

/**
 * Removes all recognized `§X` codes, returning the plain text. Used for
 * search matching, tooltips, alt text, breadcrumbs, and filenames.
 */
export function stripMinecraftCodes(input: string): string {
	return parseMinecraftFormatting(input)
		.map((segment) => segment.text)
		.join('')
}
