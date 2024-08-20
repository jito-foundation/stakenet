---
layout: default
title: Scoring System
---

# Scoring System

$`
\displaylines{
\text{mev\_commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{mev\_commission}_{t_1, t_2}) \leq \text{mev\_commission\_bps\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{mev\_commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{running\_jito\_score} = 
\begin{cases} 
1.0 & \text{if any MEV commission exists in } t_1 \text{ to } t_2 \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{mev\_commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{delinquency\_score} = 
\begin{cases} 
1.0 & \text{if } \left( \frac{\text{vote\_credits}_t}{\text{total\_blocks}_t} \right) > \text{scoring\_delinquency\_threshold\_ratio} \text{ for all } t_1 \leq t \leq t_2 \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{epoch\_credits\_range} \\
\text{and } t_2 = \text{current\_epoch} - 1
}
`$

---

$`
\displaylines{
\text{commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{commission}_{t_1, t_2}) \leq \text{commission\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{historical\_commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{historical\_commission}_{t_1, t_2}) \leq \text{historical\_commission\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{first\_reliable\_epoch} = 520 \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{blacklisted\_score} = 
\begin{cases} 
0.0 & \text{if blacklisted in current epoch} \\
1.0 & \text{otherwise}
\end{cases}
}
`$

---

$`
\displaylines{
\text{superminority\_score} = 
\begin{cases} 
0.0 & \text{if in superminority in current epoch} \\
1.0 & \text{otherwise}
\end{cases} \\
}
`$

---

$`
\displaylines{
\text{vote\_credits\_ratio} = \frac{\sum_{t=t_1}^{t_2} \text{vote\_credits}_t}{\sum_{t=t_1}^{t_2} \text{total\_blocks}_t} \\
\text{where } t_1 = \text{current\_epoch} - \text{epoch\_credits\_range} \\
\text{and } t_2 = \text{current\_epoch} - 1
}
`$

Note: total_blocks is the field in ClusterHistory that tracks how many blocks were created by the cluster in a given epoch. This represents the maximum number of vote credits that a validator can earn. Vote credits are synonymous with epoch credits.

---

$`
\displaylines{
\text{yield\_score} = \text{vote\_credits\_ratio} \times (1 - max(\text{commission}_{t_1, t_2})) \\
\text{where } t_1 = \text{current\_epoch} - \text{commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

Note: Yield score is a relative measure of the yield returned to stakers by the validator, not an exact measure of its APY.

---

$`
\displaylines{
\text{final\_score} = \text{mev\_commission\_score} \times \text{commission\_score} \times \text{historical\_commission\_score} \times \text{blacklisted\_score} \times \text{superminority\_score} \times \text{delinquency\_score} \times \text{running\_jito\_score} \times \text{yield\_score}
}
`$
