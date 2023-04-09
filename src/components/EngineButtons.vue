<script lang="ts">
export default {
  props: ["status"],
  methods: {
    sendEngineCommand(command: string) {
      if (command === "go") {
        this.isRunning = true;
      } else if (command === "stop") {
        this.isRunning = false;
      } else if (command === "restart") {
        this.isRunning = false;
      }
      this.$emit("engine-command", command);

      localStorage.setItem("status", this.isRunning);
    },
  },
  mounted() {
    this.isRunning = localStorage.getItem("status") === "true";
    console.log(this.isRunning);
  },
  data() {
    return {
      engineLog: "",
      // stopped, running
      isRunning: this.status,
    };
  },
};
</script>

<template lang="">
  <div>
    <v-row justify="center" class="mt-4 mb-4 mx-2">
      <v-col cols="4">
        <v-btn
          class="mr-2"
          outlined
          color="primary"
          block
          :disabled="isRunning"
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
          outlined
          color="primary"
          block
          :disabled="isRunning"
          @click="sendEngineCommand('restart')"
        >
          Restart
        </v-btn>
      </v-col>
    </v-row>
    <v-row>
      <v-col cols="12">
        <v-textarea
          no-resize
          hide-details
          v-model="engineLog"
          style="min-height: 100%"
          label="log"
        ></v-textarea
      ></v-col>
    </v-row>
  </div>
</template>

<style lang=""></style>
