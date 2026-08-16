# Heritage

Animus Live is inspired by [Animata](http://animata.kibu.hu/), created by Péter Németh, Gábor Papp and Bence Samu at Kitchen Budapest in 2007. Animus Live is an independent, clean-room reimplementation: it is not affiliated with the original project and contains no code derived from it. See `CONTRIBUTING.md` for the clean-room policy that keeps it that way.

Ideas Animus Live draws from the original Animata, based on its published documentation, papers, screenshots and videos:

- The **mass-spring puppet model**: puppets are simulated as networks of point masses connected by springs, giving cloth-like, physically reactive motion rather than rigid-bone deformation.
- **Joints as a graph, not a hierarchy**: a puppet's joints and constraints form an arbitrary graph rather than a strict parent-child skeleton, which is what allows the mass-spring simulation to behave the way it does.
- The **OSC-driven live workflow**: puppets are driven in real time by Open Sound Control messages from external controllers, fitting the tool into a live-performance and VJ pipeline rather than an offline animation pipeline.

Everything else in Animus Live — its architecture, code, and implementation details — is original work.
