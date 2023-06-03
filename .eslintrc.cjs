module.exports = {
	root: true,
	extends: [
		"plugin:vue/vue3-strongly-recommended",
		"eslint:recommended",
		"@vue/eslint-config-typescript/recommended"
	],
	ignorePatterns: ["tauri/*", "src-tauri/*"],
	rules: {
		// enable additional rules
		indent: ["error", "tab"], // Use tabs for indentation
	},
};
