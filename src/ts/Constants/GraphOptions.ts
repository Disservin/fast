export const options = {
	colors: ["#1D4ED8"],
	stroke: {
		curve: "straight",
		width: 2.5,
	},
	markers: {
		size: 0,
		hover: {
			size: null,
			sizeOffset: 0,
		},
	},
	chart: {
		toolbar: {
			show: false,
		},
		zoom: {
			enabled: false,
		},
		animations: {
			enabled: false,
		},
	},
	legend: {
		show: false,
		onItemHover: {
			highlightDataSeries: false,
		},
	},
	tooltip: {
		enabled: false,
	},
	xaxis: {
		labels: {
			show: false,
		},
		type: "numeric",
	},
	yaxis: {
		tickAmount: 2,
		min: -5,
		max: 5,
		labels: {
			show: false,
		},
		opacity: 0,
	},
};
