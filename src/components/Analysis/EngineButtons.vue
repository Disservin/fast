<script setup lang="ts">
// Utilities
import { onMounted, ref } from "vue";

const props = defineProps<{
	status: boolean | undefined;
	go: boolean;
}>();

const emit = defineEmits(["engine-command"]);

const isRunning = ref(props.status);
const disabled = ref(false);
const disabledGo = ref(false);

onMounted(() => {
	isRunning.value = localStorage.getItem("status") === "true";
	disabledGo.value = !props.go;
});

const sendEngineCommand = (command: string) => {
	if (command === "go") {
		isRunning.value = true;
	} else if (command === "stop") {
		isRunning.value = false;
	} else if (command === "restart") {
		disabled.value = true;
		setTimeout(() => {
			disabled.value = false;
		}, 5000);
	}
	emit("engine-command", command);

	localStorage.setItem("status", isRunning.value.toString());
};
</script>

<template lang="">
	<div class="buttons">
		<v-row justify="center" class="mt-4 mb-4 mx-2">
			<v-col cols="4">
				<v-btn
					class="mr-2"
					outlined
					color="primary"
					block
					:disabled="isRunning || disabledGo"
					@click="sendEngineCommand('go')"
				>
					Go Infinite
				</v-btn>
			</v-col>
			<v-col cols="4">
				<v-btn
					class="mr-2"
					outlined
					color="primary"
					block
					:disabled="!isRunning"
					@click="sendEngineCommand('stop')"
				>
					Stop
				</v-btn>
			</v-col>
			<v-col cols="4">
				<v-btn
					id="restart-button"
					outlined
					color="primary"
					block
					:disabled="isRunning || disabled"
					@click="sendEngineCommand('restart')"
				>
					Restart
				</v-btn>
			</v-col>
		</v-row>
	</div>
</template>

<style scoped>
.buttons {
	background-color: var(--bg-tertiary);
	margin: 5px;
}
</style>
