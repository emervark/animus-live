"""Animus Live business-plan financial model.

Prints every table in docs/business/animus-live-business-plan.md §7.
All inputs are the assumptions listed in Appendix A; change them here and
the plan's arithmetic follows. Run: python3 docs/business/model.py
"""
E = lambda x: f"{x:,.0f}"

# ---------- pricing ----------
P_STUDIO=199; P_MAINT=79; P_BUNDLE=349; P_VENUE=1200; P_TRAIN=900; P_INTEG=600; P_PACK=29
MOR_FEE=0.05          # merchant-of-record (Paddle/Lemon Squeezy) all-in
CONTINGENCY=0.08
EMP=1.338             # Estonian employer cost multiplier (33% social + 0.8% unemployment)

years=["Y1 (Sep26-Aug27)","Y2 (Sep27-Aug28)","Y3 (Sep28-Aug29)"]

# ---------- funnel ----------
downloads=[6000,22000,55000]
cum_dl=[]; t=0
for d in downloads: t+=d; cum_dl.append(t)
active_rate=[0.08,0.10,0.11]
active=[round(c*r) for c,r in zip(cum_dl,active_rate)]

# ---------- revenue units ----------
u_studio=[40,180,420]
u_maint =[0,30,150]
u_bundle=[15,55,130]
u_venue =[1,5,14]
d_train =[6,14,22]
d_integ =[10,18,24]
u_pack  =[60,260,700]
grants  =[12000,20000,15000]
sponsor =[1200,3600,7200]

rev_lines={}
rev_lines["Studio licences"]=[a*P_STUDIO for a in u_studio]
rev_lines["Maintenance renewals"]=[a*P_MAINT for a in u_maint]
rev_lines["Showmesh+Animus bundle"]=[a*P_BUNDLE for a in u_bundle]
rev_lines["Venue/institution site licences"]=[a*P_VENUE for a in u_venue]
rev_lines["Training & workshops"]=[a*P_TRAIN for a in d_train]
rev_lines["Integration & commissioned work"]=[a*P_INTEG for a in d_integ]
rev_lines["Puppet / template packs"]=[a*P_PACK for a in u_pack]
rev_lines["Grants (NLnet, Kultuurkapital, EU)"]=grants
rev_lines["Sponsorship (GitHub Sponsors, corp)"]=sponsor

rev_tot=[sum(v[i] for v in rev_lines.values()) for i in range(3)]
# product revenue = everything that goes through the store
prod_keys=["Studio licences","Maintenance renewals","Showmesh+Animus bundle",
           "Venue/institution site licences","Puppet / template packs"]
prod_rev=[sum(rev_lines[k][i] for k in prod_keys) for i in range(3)]
svc_rev=[rev_lines["Training & workshops"][i]+rev_lines["Integration & commissioned work"][i] for i in range(3)]
nonrec=[grants[i]+sponsor[i] for i in range(3)]

# ---------- costs ----------
founder_gross=[18000,36000,54000]
founder=[g*EMP for g in founder_gross]
contract=[0,15000,45000]
cost_lines={}
cost_lines["Founder compensation (incl. 33.8% employer tax)"]=founder
cost_lines["Contractors (dev, docs, video)"]=contract
cost_lines["Hardware (workstation, displays, projector, controllers)"]=[3500,2000,2500]
cost_lines["AI-assisted development tooling"]=[2400,3600,3600]
cost_lines["Code signing (EV) + Apple Developer"]=[499,499,499]
cost_lines["CI, hosting, docs site, domains"]=[600,1200,1800]
cost_lines["Accounting, legal, OU admin"]=[1500,2400,3000]
cost_lines["Trademark registration (EUIPO)"]=[1500,0,0]
cost_lines["Marketing, festivals, travel"]=[3000,7000,12000]
cost_lines["Payment processing (5% of product revenue)"]=[p*MOR_FEE for p in prod_rev]
sub=[sum(v[i] for v in cost_lines.values()) for i in range(3)]
cont=[s*CONTINGENCY for s in sub]
cost_lines["Contingency (8%)"]=cont
cost_tot=[sub[i]+cont[i] for i in range(3)]
net=[rev_tot[i]-cost_tot[i] for i in range(3)]

def table(title,lines,tot,totlabel):
    print(f"\n### {title}")
    w=max(len(k) for k in lines)+2
    print(f"{'line'.ljust(w)}{'Y1':>12}{'Y2':>12}{'Y3':>12}")
    for k,v in lines.items():
        print(f"{k.ljust(w)}{E(v[0]):>12}{E(v[1]):>12}{E(v[2]):>12}")
    print(f"{totlabel.ljust(w)}{E(tot[0]):>12}{E(tot[1]):>12}{E(tot[2]):>12}")

print("FUNNEL")
print("cum downloads",cum_dl,"active",active)
print("conv active->paid %",[round(100*(u_studio[i]+u_bundle[i])/active[i],2) for i in range(3)])
table("REVENUE",rev_lines,rev_tot,"TOTAL REVENUE")
print("  product",[E(x) for x in prod_rev]," services",[E(x) for x in svc_rev]," non-recurring",[E(x) for x in nonrec])
print("  mix % product",[round(100*prod_rev[i]/rev_tot[i]) for i in range(3)],
      "services",[round(100*svc_rev[i]/rev_tot[i]) for i in range(3)],
      "grants/spons",[round(100*nonrec[i]/rev_tot[i]) for i in range(3)])
table("COSTS",cost_lines,cost_tot,"TOTAL COSTS")
print("\nNET",[E(n) for n in net])
print("CUMULATIVE NET",[E(sum(net[:i+1])) for i in range(3)])
print("net margin %",[round(100*net[i]/rev_tot[i],1) for i in range(3)])

# ---------- Y1 quarterly cash ----------
print("\n### Y1 QUARTERLY CASH (store opens Q2)")
rev_q_split={"Studio licences":[0,.15,.35,.50],"Maintenance renewals":[0,0,0,0],
 "Showmesh+Animus bundle":[0,.13,.34,.53],"Venue/institution site licences":[0,0,.5,.5],
 "Training & workshops":[0,.17,.33,.50],"Integration & commissioned work":[.10,.20,.30,.40],
 "Puppet / template packs":[0,.10,.35,.55],"Grants (NLnet, Kultuurkapital, EU)":[0,.5,0,.5],
 "Sponsorship (GitHub Sponsors, corp)":[.1,.2,.3,.4]}
revq=[sum(rev_lines[k][0]*rev_q_split[k][q] for k in rev_lines) for q in range(4)]
# costs: founder+AI+CI+accounting spread evenly; hardware front-loaded; trademark Q1; marketing weighted late
costq=[]
for q in range(4):
    c=(founder[0]+2400+600+1500)/4
    c+=[2500,1000,0,0][q]                       # hardware
    c+=[1500,0,0,0][q]                          # trademark
    c+=[0,499,0,0][q]                           # certs
    c+=[300,600,900,1200][q]                    # marketing
    c+=revq[q]*MOR_FEE*0.75                     # fees approx on product share
    costq.append(c*(1+CONTINGENCY))
cum=0
for q in range(4):
    cum+=revq[q]-costq[q]
    print(f"Q{q+1}: revenue {E(revq[q]):>8}   costs {E(costq[q]):>8}   net {E(revq[q]-costq[q]):>8}   cumulative {E(cum):>9}")

# ---------- unit economics ----------
print("\n### UNIT ECONOMICS (Studio licence)")
gross=P_STUDIO*(1-MOR_FEE)
print("price",P_STUDIO,"net of MoR fee",round(gross,2))
print("Y3 marketing spend / paying customers acquired Y3:",round(12000/(u_studio[2]+u_bundle[2]),2),"= blended CAC")
ltv=gross+ P_MAINT*(1-MOR_FEE)*1.6
print("LTV (licence + 1.6 avg renewals):",round(ltv,2))
print("LTV/CAC:",round(ltv/(12000/(u_studio[2]+u_bundle[2])),1))

# ---------- break-even ----------
fixed_y2=cost_tot[1]-svc_rev[1]*0  # all costs fixed-ish
print("\n### BREAK-EVEN (Y2 cost base, product-only)")
print("Y2 total costs",E(cost_tot[1]),"-> licences needed at 199 net:",round(cost_tot[1]/(P_STUDIO*(1-MOR_FEE))))
print("with services+grants at plan (",E(svc_rev[1]+nonrec[1]),") licences needed:",
      round((cost_tot[1]-svc_rev[1]-nonrec[1])/(P_STUDIO*(1-MOR_FEE))))

# ---------- scenarios ----------
print("\n### SCENARIOS (Y3 revenue)")
for name,mult,gm in [("Bear",0.45,0.5),("Base",1.0,1.0),("Bull",1.9,1.3)]:
    r=prod_rev[2]*mult+svc_rev[2]*gm+nonrec[2]*gm
    c=cost_tot[2]*(0.72 if mult<1 else (1.0 if mult==1 else 1.25))
    print(f"{name:5} revenue {E(r):>9}  costs {E(c):>9}  net {E(r-c):>9}")
