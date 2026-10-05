# Smart greenhouse pilot: final report

Prepared for the board of Fjordgaarden Market Garden, December 2025.

## Summary

From March to October 2025 we ran a pilot in the large glasshouse at Fjordgaarden to find out whether cheap sensors and an automatic climate controller could save water and energy without lowering the harvest. Sixty wireless sensors measured soil moisture, air temperature, humidity and light in four growing zones, and a small computer in the packing shed collected the readings every five minutes and controlled the vents, the shade screens and the drip irrigation. Compared with the same months of 2024, the glasshouse used thirty-one percent less water and nineteen percent less heating gas, while the harvest of tomatoes, cucumbers and peppers was about the same. The pilot cost 214,000 kroner in equipment and work, and we expect it to pay for itself in a little under three years. This report describes the setup, what went well, what went wrong and what we recommend for next season.

## Background

Fjordgaarden grows vegetables for the farm shop and for three restaurants in town. The glasshouse covers about 2,400 square metres and was built in 1998. Until this year, the vents were opened by hand twice a day, the heating followed a fixed timetable, and the irrigation ran for a set number of minutes each morning, regardless of the weather. The staff knew that this wasted water on cloudy days and let the plants dry out on hot ones, but they did not have time to adjust it by hand. Rising prices for gas and water, and a new municipal limit on the amount of groundwater the farm may pump, made the board ask for a better way. A local engineering student, Freja Lund, offered to build a pilot as part of her final project.

## Goals

The pilot had four goals. First, to reduce the use of water by at least twenty percent. Second, to reduce the use of heating gas by at least ten percent. Third, to keep the harvest at least at the level of 2024. Fourth, to find out whether the staff could live with the system day to day, without an engineer on site. We also wanted to learn which sensors last a season in the warm, damp air of a glasshouse, and which do not.

## The growing zones

The glasshouse was divided into four zones of roughly equal size. Zone A, nearest the door, grew cucumbers on high wires. Zone B grew sweet peppers in pots. Zone C grew tomatoes in the soil along the south wall, and zone D, at the far end, was used for seedlings and herbs. Each zone got its own valve on the drip irrigation, so that it could be watered separately, and its own group of sensors. The vents and the shade screens are shared by the whole house and were controlled from the average of all zones.

## Sensors and hardware

We used soil moisture sensors of the capacitive kind, which do not corrode like the older resistive ones, together with combined sensors for air temperature and humidity and simple light sensors. All sensors were battery powered and sent their readings by radio to a receiver in the packing shed. The controller was a small single-board computer with a relay board that switches the irrigation valves, the vent motors and the shade screen motor. A weather station on the roof measured wind, rain and outside temperature, so that the vents could be closed before a storm. All parts were bought from ordinary electronics shops; the most expensive single item was the weather station at 6,800 kroner.

## Software

The controller ran an open-source home automation program with a set of rules written for the pilot. The rules open the vents when the inside temperature rises above the set point for the time of day, close them when it rains or the wind is strong, draw the shade screens when the light is very strong in the afternoon, and start the irrigation in a zone when the average soil moisture falls below a threshold. Every reading is stored in a database, and a dashboard on a tablet in the packing shed shows the state of each zone. The staff can override any rule with a button on the tablet, and the override stops by itself after two hours, so that nobody forgets to switch the automatic mode back on.

## Installation

The sensors were installed in the first two weeks of March, before the main planting. Each sensor was given a number and a place on a map of the glasshouse, and the soil sensors were pushed into the soil at root depth, about fifteen centimetres. The radio reception was poor in the far end of the house, behind the water tanks, so we moved the receiver to the middle of the house and put a second antenna on the roof. Wiring the relay board to the existing vent motors took longer than planned, because the old motors had no documentation and two of them turned out to be wired in reverse.

## Calibration of the soil sensors

Capacitive soil sensors give a raw number that depends on the soil type, so each sensor had to be calibrated. We took soil samples from each zone, dried them in the oven, and measured the sensor reading at known water contents from completely dry to saturated. The curves were almost straight lines for the sandy soil along the south wall and slightly curved for the richer soil in the pots. With the calibration, the sensors agreed with the oven method within three percentage points of water content, which is good enough for controlling irrigation.

## The first months

In April and May the system worked mostly as planned. The irrigation ran less often on cloudy days and more often in the sunny week at the end of May, and the water meter showed the first savings. The vents opened earlier in the morning than the staff used to open them, which kept the temperature more even and reduced the condensation on the leaves in the early hours. One problem was that the shade screens were drawn too often in May, when the plants still needed the light, so we raised the light threshold for the screens by a third.

## Problems with batteries

The batteries in the sensors were supposed to last a whole season, but eleven of the sixty sensors ran flat in July. They were the sensors that sent readings most often, because their readings changed a lot, and the sensors in the hottest part of the house, where batteries lose capacity faster. We replaced the batteries and changed the settings so that each sensor sends at most once every five minutes. For next season we recommend sensors that take larger batteries, or a few sensors with cables in the hottest places.

## The tomato problem in zone C

In the middle of July the tomatoes in zone C started to show dark, sunken patches at the bottom of the fruit, a disorder known as blossom end rot. About one fruit in six was affected over three weeks, and those fruits could not be sold. Blossom end rot is caused by a lack of calcium in the growing fruit, which almost always comes from uneven watering rather than from too little calcium in the soil. When we looked at the data, we found that the irrigation valve for zone C had stuck half-closed after a power cut on the eleventh of July, so the zone got only part of its water, while the dashboard still showed the valve as open. The soil sensors had in fact shown the soil drying out, but the alarm for low moisture had been switched off during the calibration and never switched on again. We replaced the valve, added a flow meter to each zone so that the controller can see whether water actually flows, and made the low-moisture alarm send a text message to the head gardener. The plants recovered within two weeks, and the later trusses were healthy.

## Energy results

The heating gas used from March to October fell from 41,200 to 33,400 cubic metres, a saving of nineteen percent. Most of the saving came from closing the vents earlier in the evening and drawing the energy screens at night, which the old timetable did not do. We corrected the figures for the weather using degree days from the weather station; without the correction the saving would have looked slightly larger, because the autumn of 2025 was mild.

## Water results

The water use fell from 3,950 to 2,720 cubic metres, a saving of thirty-one percent, well within the new municipal limit. The largest savings were in zone A and zone D, where the old timetable had watered far too much. In zone C the saving was smaller, partly because of the extra watering needed after the valve problem.

## Harvest

The harvest of cucumbers and peppers was within two percent of 2024. The tomato harvest was four percent lower, which is explained by the fruit lost to blossom end rot in July. Without that problem, the harvest would most likely have been slightly higher than last year, because the plants in zone C grew more evenly in the spring.

## What the staff said

We interviewed the four gardeners in October. They liked not having to open the vents by hand and found the dashboard easy to read, but they wanted fewer alarms by mail and more by text message, and a simpler way to tell the system that a zone has been replanted. They also asked for the tablet to be mounted on the wall, because it kept getting lost under seed trays.

## Costs

Equipment cost 128,000 kroner, installation and programming 64,000 kroner and calibration and the student's supervision 22,000 kroner, in all 214,000 kroner. The savings on gas and water in the pilot season were worth about 76,000 kroner at 2025 prices, so the pilot will pay back in a little under three years, sooner if prices go up.

## Recommendations

We recommend keeping the system and extending it to the small glasshouse next spring. Each zone should have a flow meter and an alarm by text message for low moisture, the batteries should be larger, and the dashboard should get a button for replanting a zone. The board should also set aside time each March for checking and recalibrating the sensors before the season starts.

## Appendix: lessons for other growers

Several growers in the region have asked whether they can copy the pilot. The short answer is yes, but plan more time than you think for the old equipment: every glasshouse has motors, valves and wiring that nobody has documented, and finding out how they work took us almost as long as installing the new parts. Buy a few spare sensors from the start, because some will fail or get damaged by a hoe or a trolley. Put the receiver where the radio reaches every corner, and test that before you mount anything. Keep the rules simple in the first season and change one thing at a time, so that you can see what each change does. Make sure every alarm reaches a person who can act on it, also at weekends, and test the alarms each month. Finally, involve the gardeners from the first day: they know the plants and the house far better than any sensor, and the system works best when it does what they would have done themselves if they had the time. Our dashboard rules and the wiring diagram are free for anyone who asks; write to the farm office and we will send them, together with the calibration sheets for sandy and rich soil and the list of parts with prices from 2025.
